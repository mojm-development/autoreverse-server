//! What the metadata editors on the book, album, books and albums pages render (legacy
//! `MetadataEditor.svelte`, `BulkEditor.svelte`, `SelectionBar.svelte`). The sheets are
//! `<template>`s filled here from the same field list the API validates against;
//! static/js/metadata/editor.js opens them and talks to `PATCH /items/{id}`, `PATCH /tracks/{id}`,
//! `POST /items/bulk[/undo]` and `POST /items/series`.

use serde_json::json;

use sqlx::PgPool;

use crate::error::WebResult;
use crate::library::bulk_metadata::{LastBulk, last_bulk_batch};
use crate::library::metadata::{FieldType, MetadataField, fields_for, item_cell};
use crate::library::queries::{Item, Track};

pub struct EditorField {
    pub wire: &'static str,
    pub label: &'static str,
    /// `text` or `number`, for `data-type` (the request's types) and the input.
    pub kind: &'static str,
    pub input_type: &'static str,
    pub step: &'static str,
    /// What the page shows now, as the input holds it.
    pub value: String,
    /// Set by hand: the scanner leaves it alone.
    pub locked: bool,
    pub hint: Option<&'static str>,
}

pub struct EditorTrack {
    pub id: i32,
    pub position: i32,
    pub title: String,
}

/// The single-item editor (book or album, with its tracks).
pub struct MetadataEditor {
    pub item_id: i32,
    pub fields: Vec<EditorField>,
    pub tracks: Vec<EditorTrack>,
    /// "Titel" for an album, "Dateien" for a book.
    pub track_label: &'static str,
}

fn kind_of(field: &MetadataField) -> (&'static str, &'static str, &'static str) {
    match field.kind {
        FieldType::Text => ("text", "text", "1"),
        FieldType::Int => ("int", "number", "1"),
        FieldType::Float => ("float", "number", "0.5"),
    }
}

impl MetadataEditor {
    pub fn new(item: &Item, tracks: &[Track], track_label: &'static str) -> MetadataEditor {
        MetadataEditor {
            item_id: item.id,
            fields: fields_for(&item.kind)
                .map(|field| {
                    let (kind, input_type, step) = kind_of(field);
                    EditorField {
                        wire: field.wire,
                        label: field.label,
                        kind,
                        input_type,
                        step,
                        value: item_cell(item, field)
                            .map(|v| v.as_text())
                            .unwrap_or_default(),
                        locked: item.locked_fields.iter().any(|l| l == field.column),
                        hint: field.hint,
                    }
                })
                .collect(),
            tracks: tracks
                .iter()
                .map(|t| EditorTrack {
                    id: t.id,
                    position: t.position,
                    title: t.title.clone().unwrap_or_default(),
                })
                .collect(),
            track_label,
        }
    }
}

pub struct BulkField {
    pub wire: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
    pub input_type: &'static str,
    pub is_text: bool,
}

/// The selection bar and the editor for many items of one kind.
pub struct BulkEditor {
    pub kind: &'static str,
    pub fields: Vec<BulkField>,
    /// How many items the current filter finds, for "Alle N auswählen".
    pub total: i64,
    /// The filter as `POST /items/bulk` takes it, when "all matching" may be chosen — `None`
    /// where the list is not one the API can filter to (the books page lists only what fits on
    /// it).
    pub filter_json: Option<String>,
    /// The user's last bulk change, still undoable after its sheet was closed.
    pub last: Option<LastBulkNotice>,
}

/// "Letzte Massenbearbeitung …" with its "Rückgängig" above the list.
pub struct LastBulkNotice {
    pub batch_id: String,
    pub text: String,
}

impl LastBulkNotice {
    pub fn from(last: &LastBulk) -> LastBulkNotice {
        let fields = if last.fields == 1 { "Feld" } else { "Felder" };
        LastBulkNotice {
            batch_id: last.batch_id.clone(),
            text: format!(
                "Letzte Massenbearbeitung: {} {fields} an {} Einträgen",
                last.fields, last.targets
            ),
        }
    }
}

impl BulkEditor {
    /// Adds the user's last bulk change, if any (see [`last_bulk_batch`]).
    pub async fn with_last(mut self, db: &PgPool, user_id: i32) -> WebResult<BulkEditor> {
        self.last = last_bulk_batch(db, user_id)
            .await?
            .as_ref()
            .map(LastBulkNotice::from);
        Ok(self)
    }

    pub fn new(kind: &'static str, total: i64, filter: Option<serde_json::Value>) -> BulkEditor {
        BulkEditor {
            kind,
            // The sort key follows the title on its own; offering it here would only invite a
            // library sorted by hand.
            fields: fields_for(kind)
                .filter(|f| f.wire != "sort_title")
                .map(|field| {
                    let (kind, input_type, _) = kind_of(field);
                    BulkField {
                        wire: field.wire,
                        label: field.label,
                        kind,
                        input_type,
                        is_text: field.kind == FieldType::Text,
                    }
                })
                .collect(),
            total,
            filter_json: filter.map(|f| f.to_string()),
            last: None,
        }
    }

    /// The albums list's filter: kind, search, artist and present-or-missing as shown.
    /// The bulk editor's "all matching" filter for the albums list. It has to select exactly
    /// what the list shows: the list by artist ignores the search and the missing filter, so
    /// this does too.
    pub fn album_filter(
        q: &str,
        artist: Option<&str>,
        genre: Option<&str>,
        missing: bool,
    ) -> serde_json::Value {
        let mut filter = json!({ "kind": "album", "missing": missing && artist.is_none() });
        if !q.is_empty() && artist.is_none() {
            filter["q"] = json!(q);
        }
        if let Some(artist) = artist {
            filter["artist"] = json!(artist);
        }
        if let Some(genre) = genre {
            filter["genre"] = json!(genre);
        }
        filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album() -> Item {
        Item {
            id: 7,
            parent_id: None,
            kind: "album".into(),
            source_path: None,
            title: "Iron Man 2".into(),
            sort_title: "iron man 2".into(),
            cover_path: None,
            artist: Some("AC/DC".into()),
            album_artist: None,
            genre: None,
            year: Some(2010),
            author: None,
            narrator: None,
            description: None,
            series: None,
            series_index: None,
            feed_url: None,
            last_checked: None,
            published_at: None,
            added_at: chrono::Utc::now(),
            missing_since: None,
            guid: None,
            chapters_url: None,
            feed_duration: None,
            keep_episodes: None,
            locked_fields: vec!["artist".into()],
        }
    }

    #[test]
    fn the_editor_offers_the_fields_of_an_album_with_their_values_and_locks() {
        let editor = MetadataEditor::new(&album(), &[], "Titel");
        let labels: Vec<&str> = editor.fields.iter().map(|f| f.label).collect();
        assert_eq!(
            labels,
            [
                "Titel",
                "Sortiertitel",
                "Interpret",
                "Album-Interpret",
                "Jahr",
                "Genre"
            ]
        );
        let artist = &editor.fields[2];
        assert_eq!((artist.value.as_str(), artist.locked), ("AC/DC", true));
        let year = &editor.fields[4];
        assert_eq!(
            (year.value.as_str(), year.input_type, year.kind),
            ("2010", "number", "int")
        );
        assert_eq!(editor.fields[3].value, "");
    }

    #[test]
    fn the_bulk_editor_leaves_out_the_sort_title() {
        let bulk = BulkEditor::new("book", 3, None);
        let wires: Vec<&str> = bulk.fields.iter().map(|f| f.wire).collect();
        assert_eq!(
            wires,
            [
                "title",
                "author",
                "narrator",
                "series",
                "series_index",
                "year"
            ]
        );
    }

    #[test]
    fn the_album_filter_is_what_the_list_shows() {
        assert_eq!(
            BulkEditor::album_filter("live", None, Some("Rock"), false),
            json!({ "kind": "album", "missing": false, "q": "live", "genre": "Rock" })
        );
        // The list by artist ignores the search and the missing filter.
        assert_eq!(
            BulkEditor::album_filter("live", Some("ACDC"), None, true),
            json!({ "kind": "album", "missing": false, "artist": "ACDC" })
        );
        assert_eq!(
            BulkEditor::album_filter("", None, None, true),
            json!({ "kind": "album", "missing": true })
        );
    }
}
