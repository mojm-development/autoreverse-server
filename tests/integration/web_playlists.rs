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
    let (cookie, user) = cookie(&app, "oliver").await;
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
    let (cookie, user) = cookie(&app, "oliver").await;
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
    let (cookie, user) = cookie(&app, "oliver").await;
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
    let (cookie, _) = cookie(&app, "oliver").await;
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
    let (cookie, user) = cookie(&app, "oliver").await;
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
    let (cookie, user) = cookie(&app, "oliver").await;
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
