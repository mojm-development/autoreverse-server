//! Hand edits to library metadata (legacy `library/metadata.ts` and `$lib/metadataFields`).
//!
//! Every field a person sets is remembered in `locked_fields`, because the scanner rewrites
//! title, author, artist, series and year from the files on every scan of a changed folder —
//! without the lock, a correction survives only until the next scan. Setting a field locks it;
//! resetting it hands the field back to the scanner. Every change is logged in `metadata_edits`
//! with the value it replaced, under one batch id, so it can be taken back.
//!
//! `locked_fields` and `metadata_edits.field` hold legacy's camelCase column names (`sortTitle`,
//! `seriesIndex`), which the API sends out as they are (`locked_fields: ["sortTitle", "title"]`).

use serde_json::Value;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};

use super::queries::{Item, Track};
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Int,
    Float,
}

/// A field the editors may touch. One definition for the API (which validates against it) and
/// the web editors (which render it), so a field cannot be in the form but refused by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataField {
    /// snake_case, as it travels over the wire.
    pub wire: &'static str,
    /// legacy's column name — what `locked_fields` and the edit log store.
    pub column: &'static str,
    /// The SQL column.
    pub sql: &'static str,
    pub kind: FieldType,
    pub label: &'static str,
    /// Which kinds of item the editor offers it for.
    pub kinds: &'static [&'static str],
    /// Longest text, in UTF-16 units like JS `length`.
    pub max: Option<usize>,
    pub hint: Option<&'static str>,
    /// `NOT NULL` in the schema: clearing it is refused instead of failing in the database.
    pub required: bool,
}

const BOTH: &[&str] = &["book", "album"];
const BOOK: &[&str] = &["book"];
const ALBUM: &[&str] = &["album"];

const fn text(
    wire: &'static str,
    column: &'static str,
    sql: &'static str,
    label: &'static str,
    kinds: &'static [&'static str],
    max: usize,
) -> MetadataField {
    MetadataField {
        wire,
        column,
        sql,
        kind: FieldType::Text,
        label,
        kinds,
        max: Some(max),
        hint: None,
        required: false,
    }
}

const fn number(
    wire: &'static str,
    column: &'static str,
    sql: &'static str,
    kind: FieldType,
    label: &'static str,
    kinds: &'static [&'static str],
) -> MetadataField {
    MetadataField {
        wire,
        column,
        sql,
        kind,
        label,
        kinds,
        max: None,
        hint: None,
        required: false,
    }
}

/// In the order the editor shows them — and the order a patch is applied and logged in.
pub const ITEM_FIELDS: &[MetadataField] = &[
    MetadataField {
        required: true,
        ..text("title", "title", "title", "Titel", BOTH, 500)
    },
    // Not `required` although the column is NOT NULL: cleared, it follows the title again.
    MetadataField {
        hint: Some("Leer lassen — folgt dem Titel, außer du willst bewusst anders einsortieren."),
        ..text(
            "sort_title",
            "sortTitle",
            "sort_title",
            "Sortiertitel",
            BOTH,
            500,
        )
    },
    text("author", "author", "author", "Autor", BOOK, 300),
    text("narrator", "narrator", "narrator", "Sprecher", BOOK, 300),
    text("artist", "artist", "artist", "Interpret", ALBUM, 300),
    text(
        "album_artist",
        "albumArtist",
        "album_artist",
        "Album-Interpret",
        ALBUM,
        300,
    ),
    text("series", "series", "series", "Serie", BOOK, 300),
    number(
        "series_index",
        "seriesIndex",
        "series_index",
        FieldType::Float,
        "Band",
        BOOK,
    ),
    number("year", "year", "year", FieldType::Int, "Jahr", BOTH),
    text("genre", "genre", "genre", "Genre", ALBUM, 100),
];

pub const TRACK_FIELDS: &[MetadataField] = &[
    text("title", "title", "title", "Titel", BOTH, 500),
    number("disc", "disc", "disc", FieldType::Int, "CD", ALBUM),
];

/// The derived sort key: it follows the title unless it was edited on its own.
pub const SORT_TITLE: &str = "sortTitle";
/// The edit-log row that records the lock list before and after.
pub const LOCK_FIELD: &str = "lockedFields";

pub fn field_by_wire(
    fields: &'static [MetadataField],
    wire: &str,
) -> Option<&'static MetadataField> {
    fields.iter().find(|f| f.wire == wire)
}

pub fn field_by_column(
    fields: &'static [MetadataField],
    column: &str,
) -> Option<&'static MetadataField> {
    fields.iter().find(|f| f.column == column)
}

/// The item fields the editor offers for a kind of item.
pub fn fields_for(kind: &str) -> impl Iterator<Item = &'static MetadataField> {
    let kind = kind.to_owned();
    ITEM_FIELDS
        .iter()
        .filter(move |f| f.kinds.contains(&kind.as_str()))
}

/// A field's value: text or a number. `None` is SQL NULL.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    Num(f64),
}

pub type Cell = Option<FieldValue>;

impl FieldValue {
    /// JS `String(value)`: what the edit log stores and an undo compares against.
    pub fn as_text(&self) -> String {
        match self {
            FieldValue::Text(s) => s.clone(),
            FieldValue::Num(n) => js_string(*n),
        }
    }
}

/// JS `String(number)` for the values a field can hold: `2011`, `1.5`.
pub fn js_string(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `asText`: the log's text form of a cell.
pub fn as_text(cell: &Cell) -> Option<String> {
    cell.as_ref().map(FieldValue::as_text)
}

/// A logged value turned back into a cell of the field's type.
pub fn from_text(field: &MetadataField, text: Option<&str>) -> Cell {
    let text = text?;
    match field.kind {
        FieldType::Text => Some(FieldValue::Text(text.to_owned())),
        _ => text.parse::<f64>().ok().map(FieldValue::Num),
    }
}

/// JS `Number(raw)` for a JSON value, near enough: numbers as they are, strings parsed (blank is
/// 0), booleans 0/1, everything else NaN.
fn js_number(raw: &Value) -> f64 {
    match raw {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.0
            } else {
                s.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}

/// JS truthiness of a JSON value.
pub fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// One wire value checked against its field (legacy `coerce`). `null` clears; blank text
/// clears too.
pub fn coerce(name: &str, field: &MetadataField, raw: &Value) -> ApiResult<Cell> {
    let cell = match raw {
        Value::Null => None,
        _ if field.kind == FieldType::Text => {
            let Value::String(s) = raw else {
                return Err(ApiError::unprocessable(format!("{name} muss Text sein")));
            };
            let value = s.trim();
            if value.is_empty() {
                None
            } else {
                if let Some(max) = field.max
                    && value.encode_utf16().count() > max
                {
                    return Err(ApiError::unprocessable(format!(
                        "{name} ist zu lang (max. {max} Zeichen)"
                    )));
                }
                Some(FieldValue::Text(value.to_owned()))
            }
        }
        _ => {
            let value = js_number(raw);
            if !value.is_finite() {
                return Err(ApiError::unprocessable(format!(
                    "{name} muss eine Zahl sein"
                )));
            }
            if field.kind == FieldType::Int {
                // Out of the column's range is no whole number it could hold either.
                if value.fract() != 0.0
                    || value < f64::from(i32::MIN)
                    || value > f64::from(i32::MAX)
                {
                    return Err(ApiError::unprocessable(format!(
                        "{name} muss eine ganze Zahl sein"
                    )));
                }
            } else if value < 0.0 {
                return Err(ApiError::unprocessable(format!(
                    "{name} darf nicht negativ sein"
                )));
            }
            Some(FieldValue::Num(value))
        }
    };
    if cell.is_none() && field.required {
        return Err(ApiError::unprocessable(format!(
            "{name} darf nicht leer sein"
        )));
    }
    Ok(cell)
}

/// A parsed `{ set, reset }`: fields to set (in field order) and fields to hand back.
#[derive(Debug, Clone, PartialEq)]
pub struct EditRequest {
    pub set: Vec<(&'static MetadataField, Cell)>,
    pub reset: Vec<&'static MetadataField>,
}

/// `set` (wire name → value) checked against `fields`.
pub fn parse_set(
    fields: &'static [MetadataField],
    raw: Option<&Value>,
) -> ApiResult<Vec<(&'static MetadataField, Cell)>> {
    let mut set = Vec::new();
    match raw {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => {
            for (name, value) in map {
                let field = field_by_wire(fields, name)
                    .ok_or_else(|| ApiError::unprocessable(format!("Unbekanntes Feld: {name}")))?;
                set.push((field, coerce(name, field, value)?));
            }
        }
        Some(_) => return Err(ApiError::unprocessable("Ungültige Anfrage")),
    }
    set.sort_by_key(|(field, _)| fields.iter().position(|f| f == *field));
    Ok(set)
}

/// `reset`: wire names handed back to the scanner.
pub fn parse_reset(
    fields: &'static [MetadataField],
    raw: Option<&Value>,
) -> ApiResult<Vec<&'static MetadataField>> {
    match raw {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(names)) => names
            .iter()
            .map(|name| {
                let name = match name {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                field_by_wire(fields, &name)
                    .ok_or_else(|| ApiError::unprocessable(format!("Unbekanntes Feld: {name}")))
            })
            .collect(),
        Some(_) => Err(ApiError::unprocessable("Ungültige Anfrage")),
    }
}

/// The body of `PATCH /items/{id}` or `PATCH /tracks/{id}`.
pub fn parse_request(fields: &'static [MetadataField], body: &Value) -> ApiResult<EditRequest> {
    let empty = serde_json::Map::new();
    let map = match body {
        Value::Object(map) => map,
        // An array is an object to JS, one without `set` or `reset`.
        Value::Array(_) => &empty,
        _ => return Err(ApiError::unprocessable("Ungültige Anfrage")),
    };
    let set = parse_set(fields, map.get("set"))?;
    let reset = parse_reset(fields, map.get("reset"))?;
    if set.is_empty() && reset.is_empty() {
        return Err(ApiError::unprocessable("Keine Änderungen angegeben"));
    }
    Ok(EditRequest { set, reset })
}

/// The lock list after an edit: set columns are locked, reset ones released. A locked title
/// carries its derived sort key; resetting the title releases both. Sorted.
pub fn next_locks(
    before: &[String],
    set_columns: &[&str],
    reset_columns: &[&str],
    with_derived: bool,
) -> Vec<String> {
    let mut locked: Vec<String> = before.to_vec();
    let add = |column: &str, locked: &mut Vec<String>| {
        if !locked.iter().any(|l| l == column) {
            locked.push(column.to_owned());
        }
    };
    for column in set_columns {
        add(column, &mut locked);
    }
    if with_derived && set_columns.contains(&"title") && !set_columns.contains(&SORT_TITLE) {
        add(SORT_TITLE, &mut locked);
    }
    for column in reset_columns {
        locked.retain(|l| l != column);
        if with_derived && *column == "title" {
            locked.retain(|l| l != SORT_TITLE);
        }
    }
    locked.sort();
    locked
}

/// One `metadata_edits` row before it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct EditRow {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

/// A patch as log rows, skipping fields that already hold the new value. The lock list is a row
/// of its own — an undo has to put the locks back, or a restored value stays frozen.
pub fn diff_to_edits(
    current: impl Fn(&MetadataField) -> Cell,
    patch: &[(&'static MetadataField, Cell)],
    locked_before: &[String],
    locked_after: &[String],
) -> (Vec<String>, Vec<EditRow>) {
    let mut changed = Vec::new();
    let mut edits = Vec::new();
    for (field, value) in patch {
        let old = current(field);
        if &old == value {
            continue;
        }
        changed.push(field.column.to_owned());
        edits.push(EditRow {
            field: field.column.to_owned(),
            old_value: as_text(&old),
            new_value: as_text(value),
        });
    }
    let before = sorted_join(locked_before);
    let after = sorted_join(locked_after);
    if !edits.is_empty() && before != after {
        edits.push(EditRow {
            field: LOCK_FIELD.to_owned(),
            old_value: Some(before),
            new_value: Some(after),
        });
    }
    (changed, edits)
}

pub fn sorted_join(locks: &[String]) -> String {
    let mut sorted = locks.to_vec();
    sorted.sort();
    sorted.join(",")
}

/// An item's current value of a field.
pub fn item_cell(item: &Item, field: &MetadataField) -> Cell {
    let text = |s: &Option<String>| s.clone().map(FieldValue::Text);
    match field.column {
        "title" => Some(FieldValue::Text(item.title.clone())),
        "sortTitle" => Some(FieldValue::Text(item.sort_title.clone())),
        "author" => text(&item.author),
        "narrator" => text(&item.narrator),
        "artist" => text(&item.artist),
        "albumArtist" => text(&item.album_artist),
        "series" => text(&item.series),
        "seriesIndex" => item.series_index.map(FieldValue::Num),
        "year" => item.year.map(|y| FieldValue::Num(f64::from(y))),
        "genre" => text(&item.genre),
        _ => None,
    }
}

/// A track's current value of a field.
pub fn track_cell(track: &Track, field: &MetadataField) -> Cell {
    match field.column {
        "title" => track.title.clone().map(FieldValue::Text),
        "disc" => track.disc.map(|d| FieldValue::Num(f64::from(d))),
        _ => None,
    }
}

/// The edited table: which rows a patch goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Item,
    Track,
}

impl Target {
    fn table(self) -> &'static str {
        match self {
            Target::Item => "items",
            Target::Track => "tracks",
        }
    }

    pub fn fields(self) -> &'static [MetadataField] {
        match self {
            Target::Item => ITEM_FIELDS,
            Target::Track => TRACK_FIELDS,
        }
    }
}

/// `UPDATE items|tracks SET <patch>, locked_fields = … WHERE id = …`.
fn update_query(
    target: Target,
    id: i32,
    patch: &[(&'static MetadataField, Cell)],
    locked: &[String],
) -> QueryBuilder<Postgres> {
    let mut query = QueryBuilder::new(format!("UPDATE {} SET locked_fields = ", target.table()));
    query.push_bind(locked.to_vec());
    for (field, value) in patch {
        query.push(format!(", {} = ", field.sql));
        match field.kind {
            FieldType::Text => query.push_bind(as_text(value)),
            FieldType::Int => query.push_bind(match value {
                Some(FieldValue::Num(n)) => Some(*n as i32),
                _ => None,
            }),
            FieldType::Float => query.push_bind(match value {
                Some(FieldValue::Num(n)) => Some(*n),
                _ => None,
            }),
        };
    }
    query.push(" WHERE id = ");
    query.push_bind(id);
    query
}

pub async fn write_patch(
    tx: &mut Transaction<'_, Postgres>,
    target: Target,
    id: i32,
    patch: &[(&'static MetadataField, Cell)],
    locked: &[String],
) -> Result<(), sqlx::Error> {
    update_query(target, id, patch, locked)
        .build()
        .execute(&mut **tx)
        .await?;
    if target == Target::Track && patch.iter().any(|(field, _)| field.column == "disc") {
        let item_id: i32 = sqlx::query_scalar("SELECT item_id FROM tracks WHERE id = $1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        order_by_disc(tx, item_id).await?;
    }
    Ok(())
}

/// Numbers an item's tracks 1..n by disc (none counts as the first), keeping their order within
/// a disc. A disc changed by hand has to move its track there: the album page heads every disc
/// change, so a track left in place would split its disc in two ("CD 1, CD 2, CD 1").
pub async fn order_by_disc(
    tx: &mut Transaction<'_, Postgres>,
    item_id: i32,
) -> Result<(), sqlx::Error> {
    // Negative first: UNIQUE(item_id, position) must hold after each statement.
    sqlx::query(
        "UPDATE tracks SET position = -ranked.n
         FROM (SELECT id, row_number() OVER (ORDER BY coalesce(disc, 1), position, id) AS n
               FROM tracks WHERE item_id = $1) AS ranked
         WHERE tracks.id = ranked.id",
    )
    .bind(item_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE tracks SET position = -position WHERE item_id = $1")
        .bind(item_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Appends log rows for one item or track under `batch_id`.
pub async fn insert_edits(
    tx: &mut Transaction<'_, Postgres>,
    batch_id: &str,
    user_id: i32,
    rows: &[(Target, i32, EditRow)],
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    // Postgres caps a statement at 65535 binds; six per row.
    for chunk in rows.chunks(5000) {
        let mut query = QueryBuilder::new(
            "INSERT INTO metadata_edits (batch_id, item_id, track_id, field, old_value, new_value, user_id) ",
        );
        query.push_values(chunk, |mut b, (target, id, edit)| {
            b.push_bind(batch_id.to_owned())
                .push_bind((*target == Target::Item).then_some(*id))
                .push_bind((*target == Target::Track).then_some(*id))
                .push_bind(edit.field.clone())
                .push_bind(edit.old_value.clone())
                .push_bind(edit.new_value.clone())
                .push_bind(user_id);
        });
        query.build().execute(&mut **tx).await?;
    }
    Ok(())
}

/// A random UUID v4, the batch id of one change.
pub fn new_batch_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random source failed");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What an edit did.
#[derive(Debug, Clone)]
pub struct EditResult<T> {
    pub row: T,
    pub batch_id: String,
    pub changed: Vec<String>,
}

/// The title's sort key: derived from a new title unless set on its own, and derived again when
/// it is cleared (the hint says "leer lassen — folgt dem Titel"; the column cannot be NULL).
/// Returns whether it was cleared — then it is handed back rather than locked.
pub fn derive_sort_title(
    patch: &mut Vec<(&'static MetadataField, Cell)>,
    current_title: &str,
) -> bool {
    let title = patch
        .iter()
        .find(|(f, _)| f.column == "title")
        .and_then(|(_, v)| as_text(v))
        .unwrap_or_else(|| current_title.to_owned());
    let derived = Some(FieldValue::Text(super::sorting::sort_key(&title)));
    if let Some(entry) = patch.iter_mut().find(|(f, _)| f.column == SORT_TITLE) {
        if entry.1.is_none() {
            entry.1 = derived;
            return true;
        }
        return false;
    }
    if patch.iter().any(|(f, _)| f.column == "title") {
        let sort_field = field_by_column(ITEM_FIELDS, SORT_TITLE).expect("sortTitle is a field");
        patch.push((sort_field, derived));
        patch.sort_by_key(|(field, _)| ITEM_FIELDS.iter().position(|f| f == *field));
    }
    false
}

/// Set and reset columns for [`next_locks`]; a cleared sort key moves from one to the other.
pub fn lock_columns(
    set: &[&MetadataField],
    reset: &[&MetadataField],
    sort_title_cleared: bool,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut set_columns = columns(set);
    let mut reset_columns = columns(reset);
    if sort_title_cleared {
        set_columns.retain(|c| *c != SORT_TITLE);
        reset_columns.push(SORT_TITLE);
    }
    (set_columns, reset_columns)
}

pub fn columns(fields: &[&MetadataField]) -> Vec<&'static str> {
    fields.iter().map(|f| f.column).collect()
}

/// `PATCH /items/{id}`: sets and locks, resets and releases, logs it all under one batch id.
pub async fn edit_item(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
    request: EditRequest,
) -> ApiResult<EditResult<Item>> {
    let mut tx = db.begin().await?;
    let existing: Item = sqlx::query_as("SELECT * FROM items WHERE id = $1 FOR UPDATE")
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("Unbekanntes Item"))?;

    let mut patch = request.set.clone();
    // The sort key follows the title, so a corrected title also sorts where it belongs.
    let cleared = derive_sort_title(&mut patch, &existing.title);
    let set_fields: Vec<&MetadataField> = request.set.iter().map(|(f, _)| *f).collect();
    let (set_columns, reset_columns) = lock_columns(&set_fields, &request.reset, cleared);
    let locked = next_locks(&existing.locked_fields, &set_columns, &reset_columns, true);
    let batch_id = new_batch_id();
    let (changed, edits) = diff_to_edits(
        |field| item_cell(&existing, field),
        &patch,
        &existing.locked_fields,
        &locked,
    );
    write_patch(&mut tx, Target::Item, item_id, &patch, &locked).await?;
    let rows: Vec<(Target, i32, EditRow)> = edits
        .into_iter()
        .map(|e| (Target::Item, item_id, e))
        .collect();
    insert_edits(&mut tx, &batch_id, user_id, &rows).await?;
    let row: Item = sqlx::query_as("SELECT * FROM items WHERE id = $1")
        .bind(item_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(EditResult {
        row,
        batch_id,
        changed,
    })
}

/// `PATCH /tracks/{id}`: a track's own title or disc, locked like an item's fields.
pub async fn edit_track(
    db: &PgPool,
    user_id: i32,
    track_id: i32,
    request: EditRequest,
) -> ApiResult<EditResult<Track>> {
    let mut tx = db.begin().await?;
    let existing: Track = sqlx::query_as("SELECT * FROM tracks WHERE id = $1 FOR UPDATE")
        .bind(track_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("Unbekannter Titel"))?;
    let set_fields: Vec<&MetadataField> = request.set.iter().map(|(f, _)| *f).collect();
    let locked = next_locks(
        &existing.locked_fields,
        &columns(&set_fields),
        &columns(&request.reset),
        false,
    );
    let batch_id = new_batch_id();
    let (changed, edits) = diff_to_edits(
        |field| track_cell(&existing, field),
        &request.set,
        &existing.locked_fields,
        &locked,
    );
    write_patch(&mut tx, Target::Track, track_id, &request.set, &locked).await?;
    let rows: Vec<(Target, i32, EditRow)> = edits
        .into_iter()
        .map(|e| (Target::Track, track_id, e))
        .collect();
    insert_edits(&mut tx, &batch_id, user_id, &rows).await?;
    let row: Track = sqlx::query_as("SELECT * FROM tracks WHERE id = $1")
        .bind(track_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(EditResult {
        row,
        batch_id,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(wire: &str) -> &'static MetadataField {
        field_by_wire(ITEM_FIELDS, wire).unwrap()
    }

    fn err(result: ApiResult<impl std::fmt::Debug>) -> String {
        result.unwrap_err().detail
    }

    #[test]
    fn text_is_trimmed_blank_clears_and_length_is_capped() {
        let author = field("author");
        assert_eq!(
            coerce("author", author, &json!("  Tolkien ")).unwrap(),
            Some(FieldValue::Text("Tolkien".into()))
        );
        assert_eq!(coerce("author", author, &json!("   ")).unwrap(), None);
        assert_eq!(coerce("author", author, &json!(null)).unwrap(), None);
        assert_eq!(
            err(coerce("author", author, &json!(5))),
            "author muss Text sein"
        );
        assert_eq!(
            err(coerce("author", author, &json!("x".repeat(301)))),
            "author ist zu lang (max. 300 Zeichen)"
        );
    }

    #[test]
    fn numbers_are_checked_like_legacy() {
        let year = field("year");
        let index = field("series_index");
        assert_eq!(
            coerce("year", year, &json!("2011")).unwrap(),
            Some(FieldValue::Num(2011.0))
        );
        assert_eq!(
            err(coerce("year", year, &json!("bald"))),
            "year muss eine Zahl sein"
        );
        assert_eq!(
            err(coerce("year", year, &json!(2011.5))),
            "year muss eine ganze Zahl sein"
        );
        assert_eq!(
            err(coerce("series_index", index, &json!(-1))),
            "series_index darf nicht negativ sein"
        );
        assert_eq!(
            coerce("series_index", index, &json!(1.5)).unwrap(),
            Some(FieldValue::Num(1.5))
        );
        // Whole numbers may be negative, only a volume may not.
        assert_eq!(
            coerce("year", year, &json!(-5)).unwrap(),
            Some(FieldValue::Num(-5.0))
        );
    }

    #[test]
    fn a_title_cannot_be_cleared() {
        assert_eq!(
            err(coerce("title", field("title"), &json!(" "))),
            "title darf nicht leer sein"
        );
    }

    #[test]
    fn requests_need_known_fields_and_some_change() {
        assert_eq!(
            err(parse_request(
                ITEM_FIELDS,
                &json!({ "set": { "kind": "album" } })
            )),
            "Unbekanntes Feld: kind"
        );
        assert_eq!(
            err(parse_request(ITEM_FIELDS, &json!({ "reset": ["nope"] }))),
            "Unbekanntes Feld: nope"
        );
        assert_eq!(
            err(parse_request(ITEM_FIELDS, &json!({}))),
            "Keine Änderungen angegeben"
        );
        assert_eq!(
            err(parse_request(ITEM_FIELDS, &json!(null))),
            "Ungültige Anfrage"
        );
        let request =
            parse_request(ITEM_FIELDS, &json!({ "set": { "year": 1, "title": "T" } })).unwrap();
        // Field order, not the order the JSON happened to have.
        assert_eq!(request.set[0].0.wire, "title");
        assert_eq!(request.set[1].0.wire, "year");
    }

    #[test]
    fn a_locked_title_carries_its_sort_key() {
        assert_eq!(
            next_locks(&[], &["title"], &[], true),
            ["sortTitle", "title"]
        );
        assert_eq!(next_locks(&[], &["title"], &[], false), ["title"]);
        assert_eq!(
            next_locks(&["sortTitle".into(), "title".into()], &[], &["title"], true),
            Vec::<String>::new()
        );
        assert_eq!(
            next_locks(&["author".into()], &["year"], &[], true),
            ["author", "year"]
        );
    }

    #[test]
    fn the_diff_skips_unchanged_values_and_logs_the_lock_change() {
        let patch = vec![
            (field("title"), Some(FieldValue::Text("Neu".into()))),
            (field("year"), Some(FieldValue::Num(2011.0))),
            (field("author"), Some(FieldValue::Text("Gleich".into()))),
        ];
        let current = |f: &MetadataField| match f.column {
            "title" => Some(FieldValue::Text("Alt".into())),
            "author" => Some(FieldValue::Text("Gleich".into())),
            _ => None,
        };
        let (changed, edits) = diff_to_edits(current, &patch, &[], &["title".into()]);
        assert_eq!(changed, ["title", "year"]);
        assert_eq!(edits[1].old_value, None);
        assert_eq!(edits[1].new_value.as_deref(), Some("2011"));
        assert_eq!(edits[2].field, LOCK_FIELD);
        assert_eq!(edits[2].old_value.as_deref(), Some(""));
        assert_eq!(edits[2].new_value.as_deref(), Some("title"));
    }

    #[test]
    fn numbers_read_like_js_strings() {
        assert_eq!(js_string(2011.0), "2011");
        assert_eq!(js_string(1.5), "1.5");
        assert_eq!(js_string(-0.0), "0");
    }

    #[test]
    fn the_sort_title_follows_a_new_title() {
        let mut patch = vec![
            (field("title"), Some(FieldValue::Text("Der Fluch".into()))),
            (field("year"), Some(FieldValue::Num(1.0))),
        ];
        assert!(!derive_sort_title(&mut patch, "alt"));
        assert_eq!(patch[1].0.column, "sortTitle");
        assert_eq!(patch[1].1, Some(FieldValue::Text("der fluch".into())));
    }

    #[test]
    fn a_cleared_sort_title_follows_the_title_again() {
        let mut patch = vec![(field("sort_title"), None)];
        assert!(derive_sort_title(&mut patch, "Der Fluch"));
        assert_eq!(patch[0].1, Some(FieldValue::Text("der fluch".into())));
        let (set, reset) = lock_columns(&[field("sort_title")], &[], true);
        assert!(set.is_empty());
        assert_eq!(reset, ["sortTitle"]);
    }

    #[test]
    fn batch_ids_are_uuids() {
        let id = new_batch_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_batch_id());
    }

    #[test]
    fn the_editor_offers_the_fields_of_the_kind() {
        let album: Vec<&str> = fields_for("album").map(|f| f.label).collect();
        assert_eq!(
            album,
            [
                "Titel",
                "Sortiertitel",
                "Interpret",
                "Album-Interpret",
                "Jahr",
                "Genre"
            ]
        );
        let book: Vec<&str> = fields_for("book").map(|f| f.wire).collect();
        assert_eq!(
            book,
            [
                "title",
                "sort_title",
                "author",
                "narrator",
                "series",
                "series_index",
                "year"
            ]
        );
    }
}
