//! Playlists in the web UI at the HTTP level (legacy library/playlists/**, the playlists e2e
//! spec where it asserts on markup, and the forms and dialog that are new here).

use axum::http::{StatusCode, header};
use sqlx::PgPool;

use autoreverse_server::library::playlists::{EntryTarget, append_entry, create_playlist};

use crate::common::{Seed, TestApp, body_text, header_str, seed, seed_track};

async fn cookie(app: &TestApp, name: &str) -> (String, i32) {
    let id = app.user(name, false).await;
    let token = app.token(id).await;
    (format!("autoreverse_session={token}"), id)
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
    let response = app.get_with_cookie(uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

async fn entry_titles(app: &TestApp, playlist: i32) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT coalesce(tracks.title, items.title) FROM playlist_entries
         LEFT JOIN tracks ON tracks.id = playlist_entries.track_id
         LEFT JOIN items ON items.id = coalesce(playlist_entries.item_id, tracks.item_id)
         WHERE playlist_id = $1 ORDER BY playlist_entries.position",
    )
    .bind(playlist)
    .fetch_all(app.db())
    .await
    .unwrap()
}

/// The e2e fixture: three albums with one titled track each, in a playlist in that order.
async fn three_tracks(app: &TestApp, user: i32) -> i32 {
    let playlist = create_playlist(app.db(), user, "E2E-Fixture-Playlist")
        .await
        .unwrap();
    for title in ["Erster Titel", "Zweiter Titel", "Dritter Titel"] {
        let album = seed(app.db(), Seed::new("album", title)).await;
        let track = seed_track(app.db(), album, 1, &format!("/p/{title}.mp3"), 200.0).await;
        sqlx::query("UPDATE tracks SET title = $2 WHERE id = $1")
            .bind(track)
            .bind(title)
            .execute(app.db())
            .await
            .unwrap();
        append_entry(app.db(), playlist.id, EntryTarget::Track(track))
            .await
            .unwrap();
    }
    playlist.id
}

#[sqlx::test]
async fn the_pages_and_forms_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in ["/library/playlists", "/library/playlists/1"] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
    for uri in [
        "/library/playlists",
        "/library/playlists/1/rename",
        "/library/playlists/1/delete",
        "/library/playlists/1/entries/1/delete",
        "/library/playlists/1/entries/1/move",
    ] {
        let response = app.post_form(uri, "name=x&position=1", None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn the_overview_shows_own_playlists_with_count_and_hours(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let html = page(&app, "/library/playlists", &cookie).await;
    assert!(html.contains("<title>Playlists · Autoreverse</title>"));
    assert!(html.contains("Noch keine Playlists."));
    // Only the one pill that works (e2e: no "Automatisch"/"Geteilt").
    assert!(html.contains(r#"<span class="pill active">Eigene</span>"#));
    assert!(!html.contains("Automatisch") && !html.contains("Geteilt"));

    let playlist = three_tracks(&app, user).await;
    let other = app.user("anna", false).await;
    create_playlist(app.db(), other, "Annas Liste")
        .await
        .unwrap();
    let html = page(&app, "/library/playlists", &cookie).await;
    assert!(html.contains(&format!(r#"href="/library/playlists/{playlist}""#)));
    assert!(html.contains("E2E-Fixture-Playlist"));
    assert!(html.contains("3 Titel · 0.2 h"), "600 s");
    assert!(!html.contains("Annas Liste"));
    assert!(html.contains(r#"href="/library/playlists" aria-current="true""#));
}

#[sqlx::test]
async fn creating_goes_to_the_new_playlist_and_checks_the_name(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let response = app
        .post_form("/library/playlists", "name=+Fahrtwind+", Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = header_str(&response, header::LOCATION).to_owned();
    let (id, name): (i32, String) =
        sqlx::query_as("SELECT id, name FROM playlists WHERE user_id = $1")
            .bind(user)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(location, format!("/library/playlists/{id}"));
    assert_eq!(name, "Fahrtwind", "trimmed");
    let html = page(&app, &location, &cookie).await;
    assert!(html.contains("<h1>Fahrtwind</h1>"));
    assert!(html.contains("0 Titel"));
    assert!(html.contains("Noch keine Titel."));
    assert!(html.contains(r#"data-action="play-playlist" disabled"#));

    for form in ["name=", "name=+++", &format!("name={}", "x".repeat(101))] {
        let response = app
            .post_form("/library/playlists", form, Some(&cookie))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{form}");
        let html = body_text(response).await;
        assert!(
            html.contains("Der Name muss 1–100 Zeichen lang sein."),
            "{form}"
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM playlists")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn the_detail_lists_entries_in_order_with_play_data(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let playlist = three_tracks(&app, user).await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Andreas Eschbach"),
            ..Seed::new("book", "Eine Billion Dollar")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/b1.mp3", 3000.0).await;
    seed_track(app.db(), book, 2, "/b2.mp3", 725.0).await;
    append_entry(app.db(), playlist, EntryTarget::Item(book))
        .await
        .unwrap();

    let html = page(&app, &format!("/library/playlists/{playlist}"), &cookie).await;
    assert!(html.contains("<title>E2E-Fixture-Playlist · Autoreverse</title>"));
    assert!(html.contains("4 Titel · 1.2 Std"));
    assert_eq!(html.matches(r#"role="row""#).count(), 4);
    let first = html.find("Erster Titel").unwrap();
    let second = html.find("Zweiter Titel").unwrap();
    let third = html.find("Dritter Titel").unwrap();
    let fourth = html.find("Eine Billion Dollar").unwrap();
    assert!(first < second && second < third && third < fourth);
    // A track entry: its title, the album as subtitle, m:ss; a whole item: author and its length.
    assert!(html.contains(r#"<span class="cell mono">3:20</span>"#));
    assert!(html.contains("Andreas Eschbach"));
    assert!(html.contains(r#"<span class="cell mono">62:05</span>"#));
    assert!(html.contains(&format!(
        r#"data-entry-item="{book}" data-entry-kind="book" data-entry-track="""#
    )));
    assert!(html.contains(r#"data-entry-kind="album" data-entry-track=""#));
    assert!(html.contains(r#"aria-label="Erster Titel abspielen""#));
    assert!(html.contains(r#"aria-label="Erster Titel nach oben" title="Nach oben" disabled"#));
    assert!(
        html.contains(r#"aria-label="Eine Billion Dollar nach unten" title="Nach unten" disabled"#)
    );
    assert!(html.contains(r#"aria-label="Zweiter Titel entfernen""#));
    assert!(html.contains(r#"data-refresh="playlist-entries""#));
    assert!(html.contains(r#"draggable="true""#));
}

#[sqlx::test]
async fn someone_elses_playlist_is_not_found(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oskar").await;
    let other = app.user("anna", false).await;
    let playlist = three_tracks(&app, other).await;
    for uri in [
        format!("/library/playlists/{playlist}"),
        "/library/playlists/abc".to_owned(),
    ] {
        let response = app.get_with_cookie(&uri, &cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert!(body_text(response).await.contains("Unbekannte Playlist"));
    }
    for uri in [
        format!("/library/playlists/{playlist}/rename"),
        format!("/library/playlists/{playlist}/delete"),
    ] {
        let response = app.post_form(&uri, "name=Meins", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    let name: String = sqlx::query_scalar("SELECT name FROM playlists WHERE id = $1")
        .bind(playlist)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(name, "E2E-Fixture-Playlist");
}

#[sqlx::test]
async fn rename_delete_move_and_remove_through_the_forms(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let playlist = three_tracks(&app, user).await;
    let back = format!("/library/playlists/{playlist}");
    let entries: Vec<i32> = sqlx::query_scalar(
        "SELECT id FROM playlist_entries WHERE playlist_id = $1 ORDER BY position",
    )
    .bind(playlist)
    .fetch_all(app.db())
    .await
    .unwrap();

    let response = app
        .post_form(&format!("{back}/rename"), "name=Pendeln", Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), back);
    assert!(
        page(&app, &back, &cookie)
            .await
            .contains("<h1>Pendeln</h1>")
    );
    let response = app
        .post_form(&format!("{back}/rename"), "name=", Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Der Name muss 1–100 Zeichen lang sein."));
    assert!(html.contains(r#"<details class="rename" open>"#));

    // "Nach unten" on the first, then "Nach oben" past the start stays at the start.
    let response = app
        .post_form(
            &format!("{back}/entries/{}/move", entries[0]),
            "position=2",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), back);
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Zweiter Titel", "Erster Titel", "Dritter Titel"]
    );
    app.post_form(
        &format!("{back}/entries/{}/move", entries[2]),
        "position=0",
        Some(&cookie),
    )
    .await;
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Dritter Titel", "Zweiter Titel", "Erster Titel"]
    );

    let response = app
        .post_form(
            &format!("{back}/entries/{}/delete", entries[1]),
            "",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Dritter Titel", "Erster Titel"]
    );
    // Already gone: back to the page all the same.
    let response = app
        .post_form(
            &format!("{back}/entries/{}/delete", entries[1]),
            "",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let response = app
        .post_form(&format!("{back}/delete"), "", Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, header::LOCATION),
        "/library/playlists"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM playlists")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn album_and_book_pages_offer_the_playlist_dialog(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    create_playlist(app.db(), user, "Zug").await.unwrap();
    create_playlist(app.db(), user, "Auto").await.unwrap();
    let other = app.user("anna", false).await;
    create_playlist(app.db(), other, "Annas Liste")
        .await
        .unwrap();
    let album = seed(app.db(), Seed::new("album", "Album")).await;
    let track = seed_track(app.db(), album, 1, "/t.mp3", 60.0).await;
    let book = seed(app.db(), Seed::new("book", "Buch")).await;

    let html = page(&app, &format!("/library/albums/{album}"), &cookie).await;
    assert!(html.contains(&format!(r#"data-playlist-pick data-pick-item="{album}""#)));
    assert!(html.contains(&format!(r#"data-playlist-pick data-pick-track="{track}""#)));
    assert!(html.contains("Titel 1 zur Playlist hinzufügen"));
    assert!(html.contains("data-playlist-picker"));
    let auto = html.find("<span>Auto</span>").unwrap();
    let zug = html.find("<span>Zug</span>").unwrap();
    assert!(auto < zug, "by name");
    assert!(!html.contains("Annas Liste"));

    let html = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert!(html.contains(&format!(r#"data-playlist-pick data-pick-item="{book}""#)));
    assert!(html.contains("<span>Zug</span>"));
}

// --- folders, sharing, cover, links (new in Rust) ------------------------------------------------

async fn set_details(
    app: &TestApp,
    playlist: i32,
    folder: Option<&str>,
    shared: bool,
    collaborative: bool,
) {
    sqlx::query("UPDATE playlists SET folder = $2, shared = $3, collaborative = $4 WHERE id = $1")
        .bind(playlist)
        .bind(folder)
        .bind(shared)
        .bind(collaborative)
        .execute(app.db())
        .await
        .unwrap();
}

/// A browser's multipart form POST with one file in the field `image`.
async fn post_file(
    app: &TestApp,
    uri: &str,
    content_type: &str,
    bytes: &[u8],
    cookie: &str,
) -> axum::http::Response<axum::body::Body> {
    let body = crate::artist_covers::multipart("image", content_type, bytes, true);
    app.send(
        axum::http::Request::post(uri)
            .header(
                header::CONTENT_TYPE,
                "multipart/form-data; boundary=XyZBoundary",
            )
            .header(header::HOST, "localhost:3000")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie)
            .body(axum::body::Body::from(body))
            .unwrap(),
    )
    .await
}

#[sqlx::test]
async fn the_new_forms_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/library/playlists/1/details",
        "/library/playlists/1/cover",
        "/library/playlists/1/cover/delete",
    ] {
        let response = app.post_form(uri, "folder=x", None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn the_overview_groups_by_folder_and_lists_what_others_shared(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let anna = app.user("anna", false).await;
    let mut ids = std::collections::HashMap::new();
    for (owner, name, folder, shared) in [
        (user, "Lose", None, false),
        (user, "Zugfahrt", Some("Reisen"), false),
        (user, "Autobahn", Some("Reisen"), true),
        (user, "Kochen", Some("Alltag"), false),
        (anna, "Annas Geteilte", Some("Annas Ordner"), true),
        (anna, "Annas Private", None, false),
    ] {
        let playlist = create_playlist(app.db(), owner, name).await.unwrap();
        set_details(&app, playlist.id, folder, shared, false).await;
        ids.insert(name, playlist.id);
    }
    let html = page(&app, "/library/playlists", &cookie).await;
    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle} fehlt"))
    };
    // Loose ones first, then the folders by name, each one a group that folds, then the others'.
    assert!(at(">Lose<") < at(r#"<span class="folder-name">Alltag</span>"#));
    assert!(at(r#"<span class="folder-name">Alltag</span>"#) < at(">Kochen<"));
    assert!(
        at(">Kochen<")
            < at(r#"<span class="folder-name">Reisen</span> <span class="count mono">2</span>"#)
    );
    assert!(at(r#"<span class="folder-name">Reisen</span>"#) < at(">Autobahn<"));
    assert!(at(">Autobahn<") < at(">Zugfahrt<"));
    assert!(at(">Zugfahrt<") < at("Von anderen geteilt"));
    assert!(at("Von anderen geteilt") < at(">Annas Geteilte<"));
    assert_eq!(html.matches(r#"<details class="folder" open>"#).count(), 2);
    assert!(html.contains("von anna · 0 Titel · 0.0 h"));
    assert!(html.contains(&format!(
        r#"href="/library/playlists/{}""#,
        ids["Annas Geteilte"]
    )));
    assert!(!html.contains("Annas Private"));
    assert!(!html.contains("Annas Ordner"), "their folders are theirs");
    assert!(!html.contains("Noch keine Playlists."));
    // "Importieren" with its file field and the place for the result.
    assert!(html.contains("data-playlist-import-open"));
    assert!(html.contains(r#"data-playlist-import hidden"#));
    assert!(html.contains("data-import-result hidden"));

    // Without anything shared the section is not there at all.
    let (annas, _) = (app.token(anna).await, ());
    let html = page(
        &app,
        "/library/playlists",
        &format!("autoreverse_session={annas}"),
    )
    .await;
    assert!(html.contains("Von anderen geteilt"), "oskar shared one");
    assert!(html.contains("von oskar"));
    sqlx::query("UPDATE playlists SET shared = false")
        .execute(app.db())
        .await
        .unwrap();
    let html = page(&app, "/library/playlists", &cookie).await;
    assert!(!html.contains("Von anderen geteilt"));
}

#[sqlx::test]
async fn the_owner_sets_description_folder_and_sharing_in_one_form(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let playlist = three_tracks(&app, user).await;
    let other = create_playlist(app.db(), user, "Andere").await.unwrap();
    set_details(&app, other.id, Some("Reisen"), false, false).await;
    let uri = format!("/library/playlists/{playlist}");

    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains("Details und Freigabe"));
    assert!(html.contains("Für alle auf diesem Server sichtbar"));
    assert!(html.contains("Andere dürfen bearbeiten"));
    assert!(
        html.contains(r#"<option value="Reisen"></option>"#),
        "known folders are offered"
    );
    assert!(html.contains(&format!(
        r#"href="/playlists/{playlist}/export?format=m3u8" download hx-boost="false""#
    )));
    assert!(html.contains("Exportieren (.m3u8)"));
    assert!(html.contains(&format!(
        r#"data-copy-link="/library/playlists/{playlist}">Link kopieren"#
    )));
    assert!(html.contains("Cover hochladen"));
    assert!(!html.contains("Cover entfernen"), "there is none yet");
    assert!(!html.contains("von oskar"), "the owner knows whose it is");

    let response = app
        .post_form(
            &format!("{uri}/details"),
            "description=Zeile+eins%0D%0AZeile+zwei&folder=+Neu+&shared=1&collaborative=1",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), uri);
    let row: (Option<String>, Option<String>, bool, bool, String) = sqlx::query_as(
        "SELECT description, folder, shared, collaborative, name FROM playlists WHERE id = $1",
    )
    .bind(playlist)
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            Some("Zeile eins\nZeile zwei".into()),
            Some("Neu".into()),
            true,
            true,
            "E2E-Fixture-Playlist".into()
        )
    );
    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains(
        r#"<p class="description">Zeile eins
Zeile zwei</p>"#
    ));
    assert!(html.contains("Playlist · Neu"));
    assert!(html.contains("geteilt, andere dürfen bearbeiten"));
    assert!(html.contains(r#"name="shared" value="1" checked"#));
    assert!(html.contains(r#"name="collaborative" value="1" checked"#));

    // An unticked box is not in the form: that switches it off; empty texts clear.
    let response = app
        .post_form(
            &format!("{uri}/details"),
            "description=&folder=",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let row: (Option<String>, Option<String>, bool, bool) = sqlx::query_as(
        "SELECT description, folder, shared, collaborative FROM playlists WHERE id = $1",
    )
    .bind(playlist)
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!(row, (None, None, false, false));

    for (form, message) in [
        (
            format!("description={}", "x".repeat(2001)),
            "Die Beschreibung darf höchstens 2000 Zeichen lang sein.",
        ),
        (
            format!("folder={}", "x".repeat(101)),
            "Der Ordnername darf höchstens 100 Zeichen lang sein.",
        ),
    ] {
        let response = app
            .post_form(&format!("{uri}/details"), &form, Some(&cookie))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains(message));
    }
}

#[sqlx::test]
async fn a_cover_is_uploaded_shown_and_removed(db: PgPool) {
    let library = crate::common::TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let (cookie, user) = cookie(&app, "oskar").await;
    let playlist = three_tracks(&app, user).await;
    let uri = format!("/library/playlists/{playlist}");
    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    let response = post_file(&app, &format!("{uri}/cover"), "image/png", PNG, &cookie).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), uri);
    let stored = library
        .data()
        .join("playlists")
        .join(format!("{playlist}.png"));
    assert!(stored.exists());
    let html = page(&app, &uri, &cookie).await;
    assert!(
        html.contains(&format!(
            r#"<img class="playlist-cover" src="/playlists/{playlist}/cover?size=384&amp;v="#
        )) || html.contains(&format!(
            r#"<img class="playlist-cover" src="/playlists/{playlist}/cover?size=384&v="#
        )) || html.contains(&format!(
            r#"<img class="playlist-cover" src="/playlists/{playlist}/cover?size=384&#38;v="#
        ))
    );
    assert!(html.contains("Cover entfernen"));
    let overview = page(&app, "/library/playlists", &cookie).await;
    assert!(overview.contains(&format!("/playlists/{playlist}/cover?size=384")));

    let response = post_file(
        &app,
        &format!("{uri}/cover"),
        "text/plain",
        b"hallo",
        &cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body_text(response)
            .await
            .contains("Nicht unterstütztes Bildformat")
    );
    assert!(stored.exists(), "a refused file leaves the cover alone");

    let response = app
        .post_form(&format!("{uri}/cover/delete"), "", Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(!stored.exists());
    assert!(!page(&app, &uri, &cookie).await.contains("playlist-cover"));
}

#[sqlx::test]
async fn another_account_sees_a_shared_playlist_without_the_owners_controls(db: PgPool) {
    let app = TestApp::new(db);
    let (_, owner) = cookie(&app, "oskar").await;
    let (cookie, _) = cookie(&app, "mara").await;
    let playlist = three_tracks(&app, owner).await;
    let uri = format!("/library/playlists/{playlist}");
    let entry: i32 = sqlx::query_scalar(
        "SELECT id FROM playlist_entries WHERE playlist_id = $1 AND position = 1",
    )
    .bind(playlist)
    .fetch_one(app.db())
    .await
    .unwrap();
    let owner_forms = [
        (format!("{uri}/rename"), "name=Meins"),
        (format!("{uri}/details"), "description=x&shared=1"),
        (format!("{uri}/cover/delete"), ""),
        (format!("{uri}/delete"), ""),
    ];
    let entry_forms = [
        (format!("{uri}/entries/{entry}/move"), "position=3"),
        (format!("{uri}/entries/{entry}/delete"), ""),
    ];

    // Private: it does not exist for her, on any route.
    let response = app.get_with_cookie(&uri, &cookie).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("Unbekannte Playlist"));
    for (form_uri, form) in owner_forms.iter().chain(&entry_forms) {
        let response = app.post_form(form_uri, form, Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{form_uri}");
    }

    // Shared: she reads and plays, and that is all.
    set_details(&app, playlist, None, true, false).await;
    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains("Playlist · von oskar"));
    assert!(html.contains("Erster Titel"));
    assert!(html.contains(r#"data-action="play-playlist""#));
    assert!(html.contains("Exportieren (.m3u8)"));
    assert!(html.contains("Link kopieren"));
    for absent in [
        "Umbenennen",
        "Löschen",
        "Details und Freigabe",
        "Cover hochladen",
        "nach oben",
        "entfernen",
        r#"draggable="true""#,
    ] {
        assert!(!html.contains(absent), "{absent}");
    }
    for (form_uri, form) in owner_forms.iter().chain(&entry_forms) {
        let response = app.post_form(form_uri, form, Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{form_uri}");
    }
    let response = post_file(
        &app,
        &format!("{uri}/cover"),
        "image/png",
        b"\x89PNG",
        &cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Erster Titel", "Zweiter Titel", "Dritter Titel"]
    );

    // Collaborative: the entries are hers to change too, the rest stays the owner's.
    set_details(&app, playlist, None, true, true).await;
    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains("Erster Titel nach unten"));
    assert!(html.contains("Erster Titel entfernen"));
    assert!(html.contains(r#"draggable="true""#));
    assert!(!html.contains("Umbenennen") && !html.contains("Details und Freigabe"));
    for (form_uri, form) in &owner_forms {
        let response = app.post_form(form_uri, form, Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{form_uri}");
    }
    let response = app
        .post_form(&entry_forms[0].0, entry_forms[0].1, Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Zweiter Titel", "Dritter Titel", "Erster Titel"]
    );
    let response = app
        .post_form(&entry_forms[1].0, entry_forms[1].1, Some(&cookie))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        entry_titles(&app, playlist).await,
        ["Zweiter Titel", "Dritter Titel"]
    );
    let name: String = sqlx::query_scalar("SELECT name FROM playlists WHERE id = $1")
        .bind(playlist)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(name, "E2E-Fixture-Playlist");
}

#[sqlx::test]
async fn the_add_dialog_offers_own_playlists_and_those_open_to_everybody(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oskar").await;
    let anna = app.user("anna", false).await;
    create_playlist(app.db(), user, "Meine").await.unwrap();
    for (name, shared, collaborative) in [
        ("Gemeinsame", true, true),
        ("Nur zum Hören", true, false),
        ("Private", false, true),
    ] {
        let playlist = create_playlist(app.db(), anna, name).await.unwrap();
        set_details(&app, playlist.id, None, shared, collaborative).await;
    }
    let album = seed(app.db(), Seed::new("album", "Album")).await;
    let html = page(&app, &format!("/library/albums/{album}"), &cookie).await;
    assert!(html.contains("<span>Meine</span>"));
    assert!(html.contains("<span>Gemeinsame (von anna)</span>"));
    assert!(!html.contains("Nur zum Hören"));
    assert!(!html.contains("Private"));
}

#[sqlx::test]
async fn pages_offer_their_link_and_a_track_link_marks_its_row(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oskar").await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("AC/DC"),
            ..Seed::new("album", "Back in Black")
        },
    )
    .await;
    let first = seed_track(app.db(), album, 1, "/m/1.mp3", 200.0).await;
    let second = seed_track(app.db(), album, 2, "/m/2.mp3", 200.0).await;
    let book = seed(app.db(), Seed::new("book", "Momo")).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Show")).await;

    let html = page(&app, &format!("/library/albums/{album}"), &cookie).await;
    assert!(html.contains(&format!(
        r#"data-copy-link="/library/albums/{album}">Link kopieren</button>"#
    )));
    assert!(!html.contains("data-linked-track"));

    // The stable address of one track: its album with `?track=<id>`.
    let html = page(
        &app,
        &format!("/library/albums/{album}?track={second}"),
        &cookie,
    )
    .await;
    assert_eq!(html.matches("data-linked-track").count(), 1);
    assert!(html.contains(&format!(
        r#"class="row clickable linked" aria-current="false" data-track-row data-track-id="{second}" data-linked-track"#
    )));
    assert!(html.contains(&format!(
        r#"class="row clickable" aria-current="false" data-track-row data-track-id="{first}">"#
    )));
    // A track that is not on this album, or no number at all, marks nothing and breaks nothing.
    for query in ["track=999999", "track=abc", "track="] {
        let html = page(&app, &format!("/library/albums/{album}?{query}"), &cookie).await;
        assert!(!html.contains("data-linked-track"), "{query}");
    }

    let html = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert!(html.contains(&format!(r#"data-copy-link="/library/books/{book}""#)));
    let html = page(&app, &format!("/library/podcasts/{podcast}"), &cookie).await;
    assert!(html.contains(&format!(r#"data-copy-link="/library/podcasts/{podcast}""#)));
    // An artist is the list of their albums.
    let html = page(&app, "/library/albums?artist=AC%2FDC", &cookie).await;
    assert!(html.contains(r#"data-copy-link="/library/albums?artist=AC%2FDC">Link kopieren"#));
    assert!(html.contains("Back in Black"));
    let html = page(&app, "/library/albums", &cookie).await;
    assert!(!html.contains("data-copy-link"));
}
