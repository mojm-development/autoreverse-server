//! Editing many items at once, the series assistant and undo (legacy `library/bulkMetadata.ts`).
//!
//! Two rules shape this: nothing is written before it has been shown (`dry_run`, the default,
//! returns the same diff the apply would perform), and everything written carries one batch id so
//! it can be taken back. A search-and-replace across two hundred titles without either is a data
//! loss waiting to happen.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path};

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::like::like_pattern;
use super::metadata::{
    Cell, EditRow, FieldType, FieldValue, ITEM_FIELDS, LOCK_FIELD, MetadataField, Target, as_text,
    derive_sort_title, diff_to_edits, field_by_column, field_by_wire, from_text, insert_edits,
    item_cell, js_truthy, lock_columns, new_batch_id, next_locks, parse_reset, parse_set,
    sorted_join, track_cell, write_patch,
};
use super::paths::get_library_paths;
use super::queries::{Item, Track};
use crate::error::{ApiError, ApiResult};
use crate::scanner::series::{SeriesSource, resolve_series};

/// Above this, an accidental "select all" would rewrite the whole library.
pub const BULK_LIMIT: usize = 2000;
/// How many rows a preview shows; the counts are always complete.
const PREVIEW_ROWS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemChange {
    pub id: i32,
    pub title: String,
    pub fields: Vec<FieldChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BulkResult {
    pub matched: usize,
    /// Items whose values change.
    pub changed: usize,
    /// Items where only the lock changes — the value already was what was asked for.
    pub locked_only: usize,
    pub changes: Vec<ItemChange>,
    pub truncated: bool,
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UndoResult {
    pub restored: usize,
    pub skipped: usize,
}

fn invalid() -> ApiError {
    ApiError::unprocessable("Ungültige Anfrage")
}

fn object(body: &Value) -> ApiResult<&Map<String, Value>> {
    body.as_object().ok_or_else(invalid)
}

/// The items a request means: `ids`, or everything a `filter` matches (top-level items only).
async fn resolve_targets(db: &PgPool, request: &Map<String, Value>) -> ApiResult<Vec<Item>> {
    let ids = request.get("ids");
    let filter = request.get("filter");
    if js_truthy(ids) && js_truthy(filter) {
        return Err(ApiError::unprocessable(
            "Entweder ids oder filter, nicht beides",
        ));
    }
    if js_truthy(ids) {
        let Some(Value::Array(raw)) = ids else {
            return Err(ApiError::unprocessable("Ungültige Item-ID"));
        };
        if raw.is_empty() {
            return Err(ApiError::unprocessable("Keine Items ausgewählt"));
        }
        let parsed: Option<Vec<i32>> = raw
            .iter()
            .map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()))
            .collect();
        let Some(parsed) = parsed else {
            return Err(ApiError::unprocessable("Ungültige Item-ID"));
        };
        if parsed.len() > BULK_LIMIT {
            return Err(ApiError::unprocessable(format!(
                "Höchstens {BULK_LIMIT} Items auf einmal"
            )));
        }
        return Ok(
            sqlx::query_as("SELECT * FROM items WHERE id = ANY($1) ORDER BY id")
                .bind(&parsed)
                .fetch_all(db)
                .await?,
        );
    }

    let Some(filter) = filter.filter(|_| js_truthy(filter)) else {
        return Err(ApiError::unprocessable("Weder ids noch filter angegeben"));
    };
    let filter = object(filter)?;
    let text = |key: &str| {
        filter
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let mut query: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT * FROM items WHERE parent_id IS NULL");
    for (key, column) in [
        ("kind", "kind"),
        ("artist", "artist"),
        ("author", "author"),
        ("series", "series"),
    ] {
        if let Some(value) = text(key) {
            query.push(format!(" AND {column} = "));
            query.push_bind(value.to_owned());
        }
    }
    if let Some(genre) = text("genre") {
        query.push(" AND lower(genre) = lower(");
        query.push_bind(genre.to_owned());
        query.push(")");
    }
    if let Some(q) = text("q") {
        let pattern = like_pattern(q);
        query.push(" AND (title ILIKE ");
        query.push_bind(pattern.clone());
        query.push(" ESCAPE '\\' OR author ILIKE ");
        query.push_bind(pattern.clone());
        query.push(" ESCAPE '\\' OR artist ILIKE ");
        query.push_bind(pattern);
        query.push(" ESCAPE '\\')");
    }
    if let Some(missing) = filter.get("missing") {
        query.push(if js_truthy(Some(missing)) {
            " AND missing_since IS NOT NULL"
        } else {
            " AND missing_since IS NULL"
        });
    }
    query.push(" ORDER BY id LIMIT ");
    query.push_bind(BULK_LIMIT as i64 + 1);
    let rows: Vec<Item> = query.build_query_as().fetch_all(db).await?;
    if rows.len() > BULK_LIMIT {
        return Err(ApiError::unprocessable(format!(
            "Der Filter trifft mehr als {BULK_LIMIT} Items"
        )));
    }
    Ok(rows)
}

/// Search and replace in one text field.
enum Replacer {
    Plain {
        field: &'static MetadataField,
        from: String,
        to: String,
    },
    Pattern {
        field: &'static MetadataField,
        pattern: Regex,
        to: String,
    },
}

impl Replacer {
    fn parse(raw: Option<&Value>) -> ApiResult<Option<Replacer>> {
        let Some(raw) = raw.filter(|r| js_truthy(Some(r))) else {
            return Ok(None);
        };
        let raw = object(raw)?;
        let name = match raw.get("field") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "undefined".into(),
        };
        let field = field_by_wire(ITEM_FIELDS, &name)
            .ok_or_else(|| ApiError::unprocessable(format!("Unbekanntes Feld: {name}")))?;
        if field.kind != FieldType::Text {
            return Err(ApiError::unprocessable("Ersetzen geht nur in Textfeldern"));
        }
        let from = match raw.get("from") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => return Err(ApiError::unprocessable("Suchtext fehlt")),
        };
        let to = raw
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if js_truthy(raw.get("regex")) {
            let pattern = Regex::new(&from)
                .map_err(|_| ApiError::unprocessable("Ungültiger regulärer Ausdruck"))?;
            return Ok(Some(Replacer::Pattern { field, pattern, to }));
        }
        Ok(Some(Replacer::Plain { field, from, to }))
    }

    fn field(&self) -> &'static MetadataField {
        match self {
            Replacer::Plain { field, .. } | Replacer::Pattern { field, .. } => field,
        }
    }

    fn apply(&self, value: &str) -> String {
        match self {
            Replacer::Plain { from, to, .. } => value.replace(from.as_str(), to),
            Replacer::Pattern { pattern, to, .. } => {
                pattern.replace_all(value, to.as_str()).into_owned()
            }
        }
    }
}

type Patch = Vec<(&'static MetadataField, Cell)>;

/// The shared write path: a per-item patch into a preview or a batch of writes. The field editor
/// and the series planner both go through here, so locking, the log and the undo behave the same
/// whichever produced the patch.
async fn apply_patches(
    db: &PgPool,
    user_id: i32,
    rows: &[Item],
    patch_for: impl Fn(&Item) -> ApiResult<Patch>,
    reset: &[&'static MetadataField],
    dry_run: bool,
) -> ApiResult<BulkResult> {
    let batch_id = new_batch_id();
    let mut changes = Vec::new();
    let mut locked_only = 0;
    let mut writes: Vec<(i32, Patch, Vec<String>)> = Vec::new();
    let mut edits: Vec<(Target, i32, EditRow)> = Vec::new();

    for row in rows {
        let mut patch = patch_for(row)?;
        // A changed title takes its sort key with it, exactly as a single edit does.
        let cleared = derive_sort_title(&mut patch, &row.title);
        let patched: Vec<&MetadataField> = patch.iter().map(|(f, _)| *f).collect();
        let (set_columns, reset_columns) = lock_columns(&patched, reset, cleared);
        let locked = next_locks(&row.locked_fields, &set_columns, &reset_columns, true);
        let (changed, diff) = diff_to_edits(
            |field| item_cell(row, field),
            &patch,
            &row.locked_fields,
            &locked,
        );
        let before = sorted_join(&row.locked_fields);
        let lock_changed = before != locked.join(",");
        if changed.is_empty() && !lock_changed {
            continue;
        }
        if changed.is_empty() {
            // The value was already right; all this does is take the field off the scanner.
            // Worth writing, not worth showing as a diff.
            locked_only += 1;
            edits.push((
                Target::Item,
                row.id,
                EditRow {
                    field: LOCK_FIELD.into(),
                    old_value: Some(before),
                    new_value: Some(locked.join(",")),
                },
            ));
        } else {
            changes.push(ItemChange {
                id: row.id,
                title: row.title.clone(),
                fields: diff
                    .iter()
                    .filter(|e| e.field != LOCK_FIELD)
                    .map(|e| FieldChange {
                        field: e.field.clone(),
                        old: e.old_value.clone(),
                        new: e.new_value.clone(),
                    })
                    .collect(),
            });
            edits.extend(diff.into_iter().map(|e| (Target::Item, row.id, e)));
        }
        writes.push((row.id, patch, locked));
    }

    let batch = if dry_run || writes.is_empty() {
        None
    } else {
        let mut tx = db.begin().await?;
        for (id, patch, locked) in &writes {
            write_patch(&mut tx, Target::Item, *id, patch, locked).await?;
        }
        insert_edits(&mut tx, &batch_id, user_id, &edits).await?;
        tx.commit().await?;
        Some(batch_id)
    };
    let changed = changes.len();
    changes.truncate(PREVIEW_ROWS);
    Ok(BulkResult {
        matched: rows.len(),
        changed,
        locked_only,
        changes,
        truncated: changed > PREVIEW_ROWS,
        batch_id: batch,
    })
}

/// `dry_run` unless it is literally `false`: an omitted flag must not write.
fn dry_run(request: &Map<String, Value>) -> bool {
    request.get("dry_run") != Some(&Value::Bool(false))
}

/// `POST /items/bulk`: `set`, `reset` and `replace` over `ids` or a `filter`.
pub async fn bulk_edit(db: &PgPool, user_id: i32, body: &Value) -> ApiResult<BulkResult> {
    let request = object(body)?;
    let set = parse_set(ITEM_FIELDS, request.get("set"))?;
    let reset = parse_reset(ITEM_FIELDS, request.get("reset"))?;
    let replacer = Replacer::parse(request.get("replace"))?;
    if set.is_empty() && reset.is_empty() && replacer.is_none() {
        return Err(ApiError::unprocessable("Keine Änderungen angegeben"));
    }
    let rows = resolve_targets(db, request).await?;
    apply_patches(
        db,
        user_id,
        &rows,
        |row| {
            let mut patch = set.clone();
            if let Some(replacer) = &replacer {
                let field = replacer.field();
                if let Some(FieldValue::Text(before)) = item_cell(row, field) {
                    let after = replacer.apply(&before);
                    if after != before {
                        let value = (!after.trim().is_empty()).then_some(FieldValue::Text(after));
                        if value.is_none() && field.column == "title" {
                            return Err(ApiError::unprocessable("title darf nicht leer sein"));
                        }
                        patch.retain(|(f, _)| f != &field);
                        patch.push((field, value));
                        patch.sort_by_key(|(f, _)| ITEM_FIELDS.iter().position(|x| x == *f));
                    }
                }
            }
            Ok(patch)
        },
        &reset,
        dry_run(request),
    )
    .await
}

/// German-ish ordering for titles: case and the common diacritics folded first, so "Ärzte" sorts
/// with the A's (`localeCompare(…, 'de')` in legacy).
pub fn compare_de(a: &str, b: &str) -> Ordering {
    fn fold(s: &str) -> String {
        s.chars()
            .flat_map(|c| {
                let lower = c.to_lowercase().next().unwrap_or(c);
                let base = match lower {
                    'ä' | 'á' | 'à' | 'â' | 'å' | 'ã' => "a",
                    'ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ø' => "o",
                    'ü' | 'ú' | 'ù' | 'û' => "u",
                    'é' | 'è' | 'ê' | 'ë' => "e",
                    'í' | 'ì' | 'î' | 'ï' => "i",
                    'ç' => "c",
                    'ñ' => "n",
                    'ß' => "ss",
                    _ => "",
                };
                if base.is_empty() {
                    vec![lower]
                } else {
                    base.chars().collect()
                }
            })
            .collect()
    }
    fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
}

/// The folders of `source` below `root`, or nothing when it is not inside it.
fn parts_below(root: &str, source: &str) -> Vec<String> {
    let resolve = crate::api::images::resolve;
    let (root, source) = (resolve(Path::new(root)), resolve(Path::new(source)));
    match source.strip_prefix(&root) {
        Ok(rest) => rest
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `POST /items/series`, the series assistant. Books whose folders never change again are
/// invisible to the scanner's own detection — `detect` runs the same rules on demand, `assign`
/// puts the selection into one series and numbers it through in the order the list shows.
pub async fn plan_series(db: &PgPool, user_id: i32, body: &Value) -> ApiResult<BulkResult> {
    let request = object(body)?;
    let assign = request.get("mode").and_then(Value::as_str) == Some("assign");
    let rows: Vec<Item> = resolve_targets(db, request)
        .await?
        .into_iter()
        .filter(|row| row.kind == "book")
        .collect();
    if rows.is_empty() {
        return Err(ApiError::unprocessable("Keine Hörbücher ausgewählt"));
    }
    let series_field = field_by_column(ITEM_FIELDS, "series").expect("series is a field");
    let index_field = field_by_column(ITEM_FIELDS, "seriesIndex").expect("seriesIndex is a field");

    if assign {
        let series = request
            .get("series")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_owned();
        if series.is_empty() {
            return Err(ApiError::unprocessable("Serienname fehlt"));
        }
        let start = match request.get("start") {
            None | Some(Value::Null) => 1.0,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
            Some(_) => f64::NAN,
        };
        if !start.is_finite() {
            return Err(ApiError::unprocessable("Startnummer muss eine Zahl sein"));
        }
        let mut ordered: Vec<&Item> = rows.iter().collect();
        ordered.sort_by(|a, b| {
            let index = |i: &Item| i.series_index.unwrap_or(f64::MAX);
            index(a)
                .total_cmp(&index(b))
                .then_with(|| compare_de(&a.sort_title, &b.sort_title))
        });
        let numbers: HashMap<i32, f64> = ordered
            .iter()
            .enumerate()
            .map(|(i, row)| (row.id, start + i as f64))
            .collect();
        return apply_patches(
            db,
            user_id,
            &rows,
            |row| {
                Ok(vec![
                    (series_field, Some(FieldValue::Text(series.clone()))),
                    (
                        index_field,
                        numbers.get(&row.id).copied().map(FieldValue::Num),
                    ),
                ])
            },
            &[],
            dry_run(request),
        )
        .await;
    }

    // The tree only means something relative to the library root: an absolute path would count
    // its own leading folders as an author and a series.
    let books_dir = get_library_paths(db).await?.books_dir;
    apply_patches(
        db,
        user_id,
        &rows,
        |row| {
            let folder_name = row
                .source_path
                .as_deref()
                .and_then(|p| Path::new(p).file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| row.title.clone());
            let parts = match (&books_dir, &row.source_path) {
                (Some(root), Some(source)) => parts_below(root, source),
                _ => Vec::new(),
            };
            // Autor/Serie/Buch: the series folder is one above the book's own.
            let parent_is_series = parts.len() >= 3;
            let guess = resolve_series(&SeriesSource {
                tag_series: None,
                tag_series_index: None,
                folder_name: &folder_name,
                parent_name: parent_is_series.then(|| parts[parts.len() - 2].as_str()),
                parent_is_series,
                title: &row.title,
                title_from_tag: true,
            });
            let Some(series) = guess.series else {
                return Ok(Vec::new());
            };
            let mut patch = vec![(series_field, Some(FieldValue::Text(series)))];
            if let Some(index) = guess.series_index {
                patch.push((index_field, Some(FieldValue::Num(index))));
            }
            Ok(patch)
        },
        &[],
        dry_run(request),
    )
    .await
}

#[derive(sqlx::FromRow)]
struct LoggedEdit {
    item_id: Option<i32>,
    track_id: Option<i32>,
    field: String,
    old_value: Option<String>,
    new_value: Option<String>,
}

fn split_locks(joined: Option<&str>) -> Vec<String> {
    joined
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The row an undo puts values back into.
enum Current {
    Item(Box<Item>),
    Track(Track),
}

impl Current {
    fn cell(&self, field: &MetadataField) -> Cell {
        match self {
            Current::Item(item) => item_cell(item, field),
            Current::Track(track) => track_cell(track, field),
        }
    }

    fn locks(&self) -> &[String] {
        match self {
            Current::Item(item) => &item.locked_fields,
            Current::Track(track) => &track.locked_fields,
        }
    }
}

/// The user's most recent edit, if it was a bulk change: one batch over two or more items or
/// tracks (a single-item edit has its own editor), made in the last 24 hours. The books and
/// albums pages offer to undo it after the sheet is closed — only this one: an older batch is
/// not "the last change" any more once something came after it.
#[derive(Debug, Clone, PartialEq)]
pub struct LastBulk {
    pub batch_id: String,
    /// Items and tracks the batch touched.
    pub targets: i64,
    /// Field changes, without the lock bookkeeping.
    pub fields: i64,
}

pub async fn last_bulk_batch(db: &PgPool, user_id: i32) -> Result<Option<LastBulk>, sqlx::Error> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "WITH last AS (
             SELECT batch_id, created_at FROM metadata_edits
             WHERE user_id = $1 ORDER BY id DESC LIMIT 1
         )
         SELECT last.batch_id,
             (SELECT count(DISTINCT (e.item_id, e.track_id)) FROM metadata_edits e
              WHERE e.batch_id = last.batch_id),
             (SELECT count(*) FROM metadata_edits e
              WHERE e.batch_id = last.batch_id AND e.field <> $2)
         FROM last
         WHERE last.created_at > now() - interval '24 hours'",
    )
    .bind(user_id)
    .bind(LOCK_FIELD)
    .fetch_optional(db)
    .await?;
    Ok(row
        .filter(|(_, targets, _)| *targets >= 2)
        .map(|(batch_id, targets, fields)| LastBulk {
            batch_id,
            targets,
            fields,
        }))
}

/// `POST /items/bulk/undo`: puts a batch back. A field someone has changed since is left alone
/// and counted as skipped — an undo must not quietly discard work that came after it.
pub async fn undo_batch(db: &PgPool, batch_id: &str) -> ApiResult<UndoResult> {
    let mut tx = db.begin().await?;
    let rows: Vec<LoggedEdit> = sqlx::query_as(
        "SELECT item_id, track_id, field, old_value, new_value FROM metadata_edits
         WHERE batch_id = $1 ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(&mut *tx)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::not_found("Unbekannte Änderung"));
    }

    let mut by_target: BTreeMap<(u8, i32), Vec<&LoggedEdit>> = BTreeMap::new();
    for row in &rows {
        let key = match (row.item_id, row.track_id) {
            (Some(id), _) => (0, id),
            (None, Some(id)) => (1, id),
            (None, None) => continue,
        };
        by_target.entry(key).or_default().push(row);
    }

    let mut restored = 0;
    let mut skipped = 0;
    for ((kind, id), edits) in by_target {
        let target = if kind == 0 {
            Target::Item
        } else {
            Target::Track
        };
        let current = match target {
            Target::Item => {
                sqlx::query_as::<_, Item>("SELECT * FROM items WHERE id = $1 FOR UPDATE")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .map(|item| Current::Item(Box::new(item)))
            }
            Target::Track => {
                sqlx::query_as::<_, Track>("SELECT * FROM tracks WHERE id = $1 FOR UPDATE")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .map(Current::Track)
            }
        };
        let Some(current) = current else {
            skipped += edits.len();
            continue;
        };

        let mut patch: Vec<(&'static MetadataField, Cell)> = Vec::new();
        let mut kept: Vec<&str> = Vec::new();
        let mut lock_edit: Option<&LoggedEdit> = None;
        for edit in &edits {
            if edit.field == LOCK_FIELD {
                lock_edit = Some(edit);
                continue;
            }
            let Some(field) = field_by_column(target.fields(), &edit.field) else {
                continue;
            };
            if as_text(&current.cell(field)) != edit.new_value {
                skipped += 1;
                kept.push(field.column);
                continue;
            }
            patch.push((field, from_text(field, edit.old_value.as_deref())));
            restored += 1;
        }

        // Locks go back to how they were, except where a field was skipped: that one belongs to
        // whoever changed it since, lock and all.
        let current_locks = current.locks();
        let mut locks = current_locks.to_vec();
        if let Some(lock_edit) = lock_edit {
            let before = split_locks(lock_edit.old_value.as_deref());
            let after = split_locks(lock_edit.new_value.as_deref());
            for column in before.iter().chain(after.iter()) {
                if kept.contains(&column.as_str()) {
                    continue;
                }
                let was = before.contains(column);
                locks.retain(|l| l != column);
                if was {
                    locks.push(column.clone());
                }
            }
            locks.sort();
            locks.dedup();
        }
        if !patch.is_empty() || locks != current_locks {
            write_patch(&mut tx, target, id, &patch, &locks).await?;
        }
    }
    sqlx::query("DELETE FROM metadata_edits WHERE batch_id = $1")
        .bind(batch_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(UndoResult { restored, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_order_folds_case_and_umlauts() {
        let mut titles = vec!["die gefährten", "Das Parfum", "der fluch", "Ärzte", "zebra"];
        titles.sort_by(|a, b| compare_de(a, b));
        assert_eq!(
            titles,
            ["Ärzte", "Das Parfum", "der fluch", "die gefährten", "zebra"]
        );
    }

    #[test]
    fn folders_count_from_the_library_root() {
        assert_eq!(
            parts_below("/books", "/books/Eschbach/Die drei Sonnen/03 - Der Fluch"),
            ["Eschbach", "Die drei Sonnen", "03 - Der Fluch"]
        );
        assert!(parts_below("/books", "/elsewhere/x").is_empty());
        assert!(parts_below("/books", "/books2/x").is_empty());
    }

    #[test]
    fn locks_split_back_into_a_list() {
        assert!(split_locks(Some("")).is_empty());
        assert_eq!(split_locks(Some("sortTitle,title")), ["sortTitle", "title"]);
    }
}
