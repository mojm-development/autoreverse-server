//! `GET /tracks/{id}/lyrics` and `has_lyrics`: lyrics from a file beside the track or from its
//! tags, read from real files. Needs ffmpeg for the audio fixtures.

use std::path::Path;

use autoreverse_server::library::paths::set_library_paths;
use axum::http::StatusCode;
use lofty::TextEncoding;
use lofty::config::WriteOptions;
use lofty::id3::v2::{
    BinaryFrame, Frame, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame,
    TimestampFormat, UnsynchronizedTextFrame,
};
use lofty::prelude::*;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TempLibrary, TestApp, body_json, make_audio, seed, seed_track};

async fn signed_in(db: PgPool) -> (TestApp, String) {
    let app = TestApp::new(db);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    (app, token)
}

/// An album with one track whose file is `path`.
async fn track_at(app: &TestApp, path: &Path) -> i32 {
    let album = seed(app.db(), Seed::new("album", "Album")).await;
    seed_track(app.db(), album, 1, path.to_str().unwrap(), 180.0).await
}

async fn lyrics(app: &TestApp, token: &str, track: i32) -> (StatusCode, Value) {
    let response = app
        .get(&format!("/tracks/{track}/lyrics"), Some(token))
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

async fn flag(app: &TestApp, track: i32) -> bool {
    sqlx::query_scalar("SELECT has_lyrics FROM tracks WHERE id = $1")
        .bind(track)
        .fetch_one(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn a_timed_file_beside_the_track_is_synced_lyrics(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("01 Lied.mp3");
    std::fs::write(&audio, b"-").unwrap();
    std::fs::write(
        dir.path().join("01 Lied.lrc"),
        "[ar:Die Band]\n[offset:-500]\n[00:10.00][00:40.00]Refrain\n[00:20.50]Strophe\n[00:30.00]\n",
    )
    .unwrap();
    let track = track_at(&app, &audio).await;

    let (status, body) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "synced": true,
            "source": "file",
            "lines": [
                {"t": 10.5, "text": "Refrain"},
                {"t": 21, "text": "Strophe"},
                {"t": 30.5, "text": ""},
                {"t": 40.5, "text": "Refrain"},
            ]
        })
    );
}

#[sqlx::test]
async fn a_plain_text_file_is_unsynced_and_keeps_its_verses(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("lied.flac");
    std::fs::write(&audio, b"-").unwrap();
    // Latin-1, as an old Windows editor saved it.
    std::fs::write(
        dir.path().join("lied.txt"),
        b"Gr\xFC\xDFe aus der Ferne\r\nZweite Zeile\r\n\r\nNeue Strophe\r\n",
    )
    .unwrap();
    let track = track_at(&app, &audio).await;

    let (status, body) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "synced": false,
            "source": "file",
            "lines": [
                {"t": null, "text": "Grüße aus der Ferne"},
                {"t": null, "text": "Zweite Zeile"},
                {"t": null, "text": ""},
                {"t": null, "text": "Neue Strophe"},
            ]
        })
    );
}

#[sqlx::test]
async fn no_text_and_no_track_are_two_different_404s(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("stumm.mp3");
    make_audio(&audio, &[("title", "Stumm")]);
    let track = track_at(&app, &audio).await;

    let (status, body) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"detail": "Kein Songtext vorhanden"}));
    for unknown in ["9999", "abc"] {
        let response = app
            .get(&format!("/tracks/{unknown}/lyrics"), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(response).await,
            json!({"detail": "Unbekannter Track"})
        );
    }
    let response = app.get(&format!("/tracks/{track}/lyrics"), None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // A file that is too large to be a song's text is not one.
    std::fs::write(dir.path().join("stumm.txt"), "la\n".repeat(100_000)).unwrap();
    let (status, _) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn embedded_lyrics_are_read_from_the_tags_of_each_format(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let text = "Erste Zeile\nZweite Zeile\n\nNeue Strophe";
    let plain = json!([
        {"t": null, "text": "Erste Zeile"},
        {"t": null, "text": "Zweite Zeile"},
        {"t": null, "text": ""},
        {"t": null, "text": "Neue Strophe"},
    ]);
    // ffmpeg writes these as the Vorbis comments, a free-form ID3v2 frame (TXXX:lyrics-deu) and
    // an MP4 ©lyr atom.
    for (file, key) in [
        ("vorbis.flac", "LYRICS"),
        ("vorbis.ogg", "UNSYNCEDLYRICS"),
        ("id3.mp3", "lyrics-deu"),
        ("mp4.m4a", "lyrics"),
    ] {
        let audio = dir.path().join(file);
        make_audio(&audio, &[("title", "Lied"), (key, text)]);
        let track = track_at(&app, &audio).await;
        let (status, body) = lyrics(&app, &token, track).await;
        assert_eq!(status, StatusCode::OK, "{file}");
        assert_eq!(body["source"], "tags", "{file}");
        assert_eq!(body["synced"], false, "{file}");
        assert_eq!(body["lines"], plain, "{file}");
    }

    // Embedded text that carries LRC stamps counts as timed, whatever field it sits in.
    let timed = json!([{"t": 1, "text": "Eins"}, {"t": 2.5, "text": "Zwei"}]);
    for (file, key) in [("lrc.flac", "LYRICS"), ("synced.flac", "SYNCEDLYRICS")] {
        let audio = dir.path().join(file);
        make_audio(&audio, &[(key, "[00:02.50]Zwei\n[00:01.00]Eins")]);
        let track = track_at(&app, &audio).await;
        let (_, body) = lyrics(&app, &token, track).await;
        assert_eq!(body["synced"], true, "{file}");
        assert_eq!(body["source"], "tags", "{file}");
        assert_eq!(body["lines"], timed, "{file}");
    }

    // The file beside the track goes before its tags.
    std::fs::write(dir.path().join("lrc.txt"), "Von Hand").unwrap();
    let audio = dir.path().join("lrc.flac");
    let track: i32 = sqlx::query_scalar("SELECT id FROM tracks WHERE path = $1")
        .bind(audio.to_str().unwrap())
        .fetch_one(app.db())
        .await
        .unwrap();
    let (_, body) = lyrics(&app, &token, track).await;
    assert_eq!(body["source"], "file");
    assert_eq!(body["lines"], json!([{"t": null, "text": "Von Hand"}]));
}

#[sqlx::test]
async fn an_id3_uslt_frame_in_utf16_is_unsynced_lyrics(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("uslt.mp3");
    make_audio(&audio, &[("title", "Lied")]);
    let mut tag = Id3v2Tag::new();
    tag.set_title("Lied".to_owned());
    tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
        TextEncoding::UTF16,
        *b"deu",
        String::new(),
        "Über sieben Brücken\rmusst du gehn".to_owned(),
    )));
    tag.save_to_path(&audio, WriteOptions::default()).unwrap();
    let track = track_at(&app, &audio).await;

    let (status, body) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "synced": false,
            "source": "tags",
            "lines": [
                {"t": null, "text": "Über sieben Brücken"},
                {"t": null, "text": "musst du gehn"},
            ]
        })
    );
}

#[sqlx::test]
async fn an_id3_sylt_frame_is_synced_lyrics(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("sylt.mp3");
    make_audio(&audio, &[("title", "Lied")]);
    // UTF-16, as the taggers on Windows write the frame.
    let frame = SynchronizedTextFrame::new(
        TextEncoding::UTF16,
        *b"deu",
        TimestampFormat::MS,
        SyncTextContentType::Lyrics,
        None,
        vec![
            (12_500, "\nÜber den Wolken".to_owned()),
            (1000, "Erste".to_owned()),
        ],
    );
    let bytes = frame.as_bytes(WriteOptions::default()).unwrap();
    let mut tag = Id3v2Tag::new();
    tag.set_title("Lied".to_owned());
    tag.insert(Frame::Binary(BinaryFrame::new(
        FrameId::Valid("SYLT".into()),
        bytes,
    )));
    tag.save_to_path(&audio, WriteOptions::default()).unwrap();
    let track = track_at(&app, &audio).await;

    let (status, body) = lyrics(&app, &token, track).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "synced": true,
            "source": "tags",
            "lines": [{"t": 1, "text": "Erste"}, {"t": 12.5, "text": "Über den Wolken"}]
        })
    );
}

#[sqlx::test]
async fn the_track_shapes_say_whether_there_are_lyrics(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("a.mp3");
    std::fs::write(&audio, b"-").unwrap();
    let album = seed(app.db(), Seed::new("album", "Album")).await;
    let track = seed_track(app.db(), album, 1, audio.to_str().unwrap(), 180.0).await;
    sqlx::query("UPDATE tracks SET title = 'Antenne'")
        .execute(app.db())
        .await
        .unwrap();

    let shapes = async |expected: bool| {
        let play = body_json(
            app.post_json(&format!("/play/{album}"), json!({}), Some(&token))
                .await,
        )
        .await;
        assert_eq!(play["tracks"][0]["has_lyrics"], expected);
        let item = body_json(app.get(&format!("/items/{album}"), Some(&token)).await).await;
        assert_eq!(item["tracks"][0]["has_lyrics"], expected);
        let one = body_json(app.get(&format!("/tracks/{track}"), Some(&token)).await).await;
        assert_eq!(one["has_lyrics"], expected);
        let search = body_json(app.get("/search?q=Antenne", Some(&token)).await).await;
        assert_eq!(search["tracks"][0]["has_lyrics"], expected);
    };
    shapes(false).await;

    // Asking for the text finds the file and puts the flag right without a scan …
    std::fs::write(dir.path().join("a.txt"), "Text").unwrap();
    assert_eq!(lyrics(&app, &token, track).await.0, StatusCode::OK);
    assert!(flag(&app, track).await);
    shapes(true).await;
    // … and takes it back when the text is gone, but not while the audio file cannot be seen.
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    assert_eq!(lyrics(&app, &token, track).await.0, StatusCode::NOT_FOUND);
    assert!(!flag(&app, track).await);
    sqlx::query("UPDATE tracks SET has_lyrics = true")
        .execute(app.db())
        .await
        .unwrap();
    std::fs::remove_file(&audio).unwrap();
    assert_eq!(lyrics(&app, &token, track).await.0, StatusCode::NOT_FOUND);
    assert!(flag(&app, track).await);
}

async fn scan(app: &TestApp, token: &str) {
    let response = app.post_raw("/scan", "", Some(token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let status = app.wait_for_scan().await;
    assert_eq!(status.last_error, None);
}

/// `has_lyrics` of the scanned tracks by file name.
async fn flags(app: &TestApp) -> Vec<(String, bool)> {
    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT path, has_lyrics FROM tracks ORDER BY path")
            .fetch_all(app.db())
            .await
            .unwrap();
    rows.into_iter()
        .map(|(path, has)| {
            let name = Path::new(&path).file_name().unwrap().to_str().unwrap();
            (name.to_owned(), has)
        })
        .collect()
}

#[sqlx::test]
async fn the_scan_keeps_the_flag_also_for_folders_it_does_not_read_again(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let admin = app.user("admin", true).await;
    let token = app.token(admin).await;
    set_library_paths(
        app.db(),
        library.books().to_str().unwrap(),
        library.music().to_str().unwrap(),
    )
    .await
    .unwrap();
    let album = library.music().join("Band/Album");
    make_audio(
        &album.join("01 Ohne.mp3"),
        &[("title", "Ohne"), ("track", "1")],
    );
    make_audio(
        &album.join("02 Datei.mp3"),
        &[("title", "Datei"), ("track", "2")],
    );
    make_audio(
        &album.join("03 Tags.flac"),
        &[("title", "Tags"), ("track", "3"), ("LYRICS", "Im Tag")],
    );
    std::fs::write(album.join("02 Datei.lrc"), "[00:01.00]Neben der Datei").unwrap();

    let names = |a: bool, b: bool, c: bool| {
        vec![
            ("01 Ohne.mp3".to_owned(), a),
            ("02 Datei.mp3".to_owned(), b),
            ("03 Tags.flac".to_owned(), c),
        ]
    };
    scan(&app, &token).await;
    assert_eq!(flags(&app).await, names(false, true, true));

    // A text added beside a file changes no audio file: the folder counts as unchanged and is
    // not read again, and the flag follows all the same. So does a text that is taken away.
    std::fs::write(album.join("01 Ohne.txt"), "Jetzt mit Text").unwrap();
    std::fs::remove_file(album.join("02 Datei.lrc")).unwrap();
    scan(&app, &token).await;
    assert_eq!(app.wait_for_scan().await.last_report.unwrap().unchanged, 1);
    assert_eq!(flags(&app).await, names(true, false, true));

    // A library from before the column: nothing is known about any track. The next scan fills
    // the flags in although every folder is unchanged — nobody has to rescan anything by hand.
    sqlx::query("UPDATE tracks SET has_lyrics = false, lyrics_embedded = NULL")
        .execute(app.db())
        .await
        .unwrap();
    scan(&app, &token).await;
    assert_eq!(app.wait_for_scan().await.last_report.unwrap().unchanged, 1);
    assert_eq!(flags(&app).await, names(true, false, true));
    let examined: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tracks WHERE lyrics_embedded IS NOT NULL")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(examined, 3);

    // A file whose tags changed is examined again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    make_audio(
        &album.join("03 Tags.flac"),
        &[
            ("title", "Tags"),
            ("track", "3"),
            ("comment", "ohne Text, länger"),
        ],
    );
    scan(&app, &token).await;
    assert_eq!(flags(&app).await, names(true, false, false));
}
