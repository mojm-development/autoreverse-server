//! The music pages at the HTTP level: albums, album detail, artists, artist detail and the
//! sidebar's music counts (legacy library/albums/**, library/artists/** page servers).

use axum::http::{StatusCode, header};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_text, header_str, seed, seed_track};

async fn cookie(app: &TestApp, name: &str, is_admin: bool) -> String {
    let id = app.user(name, is_admin).await;
    let token = app.token(id).await;
    format!("autoreverse_session={token}")
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> (StatusCode, String) {
    let response = app.get_with_cookie(uri, cookie).await;
    let status = response.status();
    (status, body_text(response).await)
}

fn album<'a>(title: &'a str, artist: &'a str) -> Seed<'a> {
    Seed {
        artist: Some(artist),
        ..Seed::new("album", title)
    }
}

async fn set_year(db: &PgPool, id: i32, year: i32) {
    sqlx::query("UPDATE items SET year = $2 WHERE id = $1")
        .bind(id)
        .bind(year)
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn music_pages_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/library/albums",
        "/library/albums/1",
        "/library/albums/more",
        "/library/artists",
        "/library/artists/Kraftwerk",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn the_albums_page_lists_present_albums_with_counts(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    let autobahn = seed(
        app.db(),
        Seed {
            cover_path: Some("/m/cover.jpg"),
            ..album("Autobahn", "Kraftwerk")
        },
    )
    .await;
    set_year(app.db(), autobahn, 1974).await;
    seed_track(app.db(), autobahn, 1, "/m/1.mp3", 1350.0).await;
    seed(app.db(), album("Nordlicht", "Ansa Volt")).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..album("Verschwunden", "Ansa Volt")
        },
    )
    .await;
    seed(app.db(), Seed::new("book", "Kein Album")).await;

    let (status, html) = page(&app, "/library/albums", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Alben · Autoreverse</title>"));
    assert!(html.contains("Autobahn") && html.contains("Nordlicht"));
    assert!(!html.contains("Verschwunden") && !html.contains("Kein Album"));
    assert!(html.contains(&format!(r#"<img src="/items/{autobahn}/cover?size=384""#)));
    assert!(html.contains(&format!(r#"href="/library/albums/{autobahn}""#)));
    // Heading count and the sidebar's music counts.
    assert!(html.contains(r#"<span class="count mono">2</span>"#));
    assert!(html.contains(r#"href="/library/albums" aria-current="true""#));
    assert!(html.contains(r#">Interpreten <span class="count mono">2</span></a"#));
    // A short list has nothing more to load.
    assert!(!html.contains("class=\"sentinel\""));

    let (_, list) = page(&app, "/library/albums?view=list", &cookie).await;
    assert!(list.contains(r#"role="table" aria-label="Alben""#));
    assert!(list.contains("1974"));
    assert!(list.contains("23 min"));
}

#[sqlx::test]
async fn the_albums_page_searches_sorts_and_filters(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    seed(app.db(), album("Nordlicht", "Ansa Volt")).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..album("Verschwunden", "Ansa Volt")
        },
    )
    .await;

    let (_, html) = page(&app, "/library/albums?q=nord", &cookie).await;
    assert!(html.contains("Nordlicht") && !html.contains("Autobahn"));
    assert!(html.contains("Suche: nord"));
    assert!(html.contains(r#"aria-label="Suche aufheben""#));

    let (_, html) = page(&app, "/library/albums?q=xyz", &cookie).await;
    assert!(html.contains("Nichts gefunden für „xyz“."));

    let (_, html) = page(&app, "/library/albums?sort=added", &cookie).await;
    assert!(html.contains(r#"href="/library/albums?sort=added" aria-current="true">Zuletzt dazu"#));

    let (_, html) = page(&app, "/library/albums?missing=true", &cookie).await;
    assert!(html.contains("<title>Fehlende Alben · Autoreverse</title>"));
    assert!(html.contains("Verschwunden") && !html.contains("Nordlicht"));

    let (_, html) = page(&app, "/library/albums?artist=Kraftwerk", &cookie).await;
    assert!(html.contains("Alben von Kraftwerk"));
    assert!(html.contains("Interpret: Kraftwerk"));
    assert!(html.contains("Autobahn") && !html.contains("Nordlicht"));
}

#[sqlx::test]
async fn an_empty_library_says_so(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    let (_, html) = page(&app, "/library/albums", &cookie).await;
    assert!(html.contains("Noch keine Alben in der Bibliothek."));
    let (_, html) = page(&app, "/library/artists", &cookie).await;
    assert!(html.contains("Noch keine Interpreten in der Bibliothek."));
}

#[sqlx::test]
async fn a_long_list_loads_the_rest_page_by_page(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    sqlx::query(
        "INSERT INTO items (kind, title, sort_title, artist)
         SELECT 'album', 'Album ' || n, 'album ' || n, 'X' FROM generate_series(1, 150) AS n",
    )
    .execute(app.db())
    .await
    .unwrap();

    let (_, html) = page(&app, "/library/albums?view=list", &cookie).await;
    assert!(html.contains(r#"<span class="count mono">150</span>"#));
    assert_eq!(html.matches(r#"class="row""#).count(), 100);
    assert!(html.contains(r#"hx-get="/library/albums/more?view=list&#38;offset=100""#));
    // Loads ahead of the end (CSS lifts the trip wire), says so while loading, and every row
    // carries the key the page uses to drop a repeated album.
    assert!(html.contains(r#"hx-trigger="intersect once""#));
    assert!(html.contains(r#"data-loading-label="Weitere Alben werden geladen""#));
    assert!(html.contains("data-paged-list") && html.contains("data-paged-status"));
    assert_eq!(html.matches("data-entry-key=").count(), 100);

    let (status, chunk) = page(&app, "/library/albums/more?view=list&offset=100", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(chunk.matches(r#"class="row""#).count(), 50);
    // Numbering continues and natural sort carries over the page boundary.
    assert!(chunk.contains(r#"<span class="number">101</span>"#));
    assert_eq!(chunk.matches("data-entry-key=").count(), 50);
    assert!(chunk.contains("Album 101") && chunk.contains("Album 150"));
    assert!(!chunk.contains("<html") && !chunk.contains("hx-get"));
}

#[sqlx::test]
async fn the_album_page_shows_the_track_list(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    let id = seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    set_year(app.db(), id, 1974).await;
    let first = seed_track(app.db(), id, 1, "/m/1.mp3", 1350.0).await;
    seed_track(app.db(), id, 2, "/m/2.mp3", 2280.0).await;
    sqlx::query("UPDATE tracks SET title = 'Autobahn' WHERE id = $1")
        .bind(first)
        .execute(app.db())
        .await
        .unwrap();

    let (status, html) = page(&app, &format!("/library/albums/{id}"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Autobahn · Autoreverse</title>"));
    assert!(html.contains("Album · 1974"));
    assert!(html.contains("Kraftwerk · 2 Titel · 1.0 Std"));
    assert!(html.contains(r#"role="table" aria-label="Titel""#));
    assert!(html.contains(r#"<span class="title">Autobahn</span>"#));
    // An untitled track is numbered.
    assert!(html.contains(r#"<span class="title">Titel 2</span>"#));
    assert!(html.contains("23 min") && html.contains("38 min"));
    // The sidebar marks the music section.
    assert!(html.contains("--a: var(--music)"));
}

#[sqlx::test]
async fn an_unknown_album_is_a_404_page(db: PgPool) {
    let app = TestApp::new(db);
    let cookie = cookie(&app, "oliver", false).await;
    let book = seed(app.db(), Seed::new("book", "Ein Buch")).await;
    for uri in [
        "/library/albums/999999".to_owned(),
        "/library/albums/abc".to_owned(),
        format!("/library/albums/{book}"),
    ] {
        let (status, html) = page(&app, &uri, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("Unbekanntes Album"), "{uri}");
    }
}

#[sqlx::test]
async fn the_artists_page_shows_pictures_initials_and_counts(db: PgPool) {
    let app = TestApp::new(db);
    // The very first user is always an admin.
    let admin = self::cookie(&app, "admin", true).await;
    let cookie = cookie(&app, "oliver", false).await;
    let autobahn = seed(
        app.db(),
        Seed {
            cover_path: Some("/m/autobahn.jpg"),
            ..album("Autobahn", "Kraftwerk")
        },
    )
    .await;
    seed(app.db(), album("Radioaktivität", "Kraftwerk")).await;
    seed(app.db(), album("Nordlicht", "Ansa Volt")).await;
    seed(app.db(), album("13", "Die Ärzte")).await;
    sqlx::query("INSERT INTO artist_covers (artist, image_path) VALUES ('Die Ärzte', '/x.png')")
        .execute(app.db())
        .await
        .unwrap();

    let (status, html) = page(&app, "/library/artists", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Interpreten · Autoreverse</title>"));
    assert!(html.contains(r#"Interpreten <span class="count mono">3</span>"#));
    assert!(html.contains("2 Alben") && html.contains("1 Album"));
    // The fallback album's cover, initials without a picture, the upload under the artist's URL.
    assert!(html.contains(&format!(r#"<img src="/items/{autobahn}/cover?size=384""#)));
    assert!(html.contains(r#"<span class="initials">AV</span>"#));
    assert!(html.contains(r#"<img src="/artists/Die%20%C3%84rzte/image?v="#));
    assert!(html.contains(r#"href="/library/albums?artist=Ansa%20Volt""#));
    // Only admins get the picture picker.
    assert!(!html.contains("Bild für Kraftwerk wählen"));

    let (_, html) = page(&app, "/library/artists", &admin).await;
    assert!(
        html.contains(
            r#"href="/library/artists/Kraftwerk" aria-label="Bild für Kraftwerk wählen""#
        )
    );
}

#[sqlx::test]
async fn the_artist_page_is_for_admins_and_marks_the_current_choice(db: PgPool) {
    let app = TestApp::new(db);
    // The very first user is always an admin.
    let admin = cookie(&app, "admin", true).await;
    let user = cookie(&app, "oliver", false).await;
    let response = app
        .get_with_cookie("/library/artists/Kraftwerk", &user)
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library");

    let old = seed(
        app.db(),
        Seed {
            cover_path: Some("/m/a.jpg"),
            ..album("Autobahn", "Kraftwerk")
        },
    )
    .await;
    let new = seed(
        app.db(),
        Seed {
            cover_path: Some("/m/b.jpg"),
            ..album("Tour de France", "Kraftwerk")
        },
    )
    .await;
    set_year(app.db(), old, 1974).await;
    set_year(app.db(), new, 2003).await;
    sqlx::query("INSERT INTO artist_covers (artist, item_id) VALUES ('Kraftwerk', $1)")
        .bind(old)
        .execute(app.db())
        .await
        .unwrap();

    let (status, html) = page(&app, "/library/artists/Kraftwerk", &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1>Kraftwerk</h1>"));
    assert!(html.contains("Zurück zu den Interpreten"));
    // Newest first; the chosen album is marked.
    let tour = html.find("Tour de France").unwrap();
    let autobahn = html.find("<span class=\"title\">Autobahn").unwrap();
    assert!(tour < autobahn);
    assert!(html.contains("1974 · gewählt"));
    assert_eq!(
        html.matches(r#"class="album bare" aria-pressed="true""#)
            .count(),
        1
    );

    let (_, html) = page(&app, "/library/artists/Niemand", &admin).await;
    assert!(html.contains("Für diesen Interpreten ist kein Album in der Bibliothek."));
}

#[sqlx::test]
async fn the_album_page_names_each_tracks_artist_and_links_the_genre(db: PgPool) {
    let app = TestApp::new(db);
    let user = cookie(&app, "oliver", false).await;
    let id = seed(app.db(), album("Bravo Hits", "Verschiedene Interpreten")).await;
    let own = seed_track(app.db(), id, 1, "/m/1.mp3", 200.0).await;
    seed_track(app.db(), id, 2, "/m/2.mp3", 200.0).await;
    sqlx::query("UPDATE tracks SET artist = 'Nena' WHERE id = $1")
        .bind(own)
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query("UPDATE items SET genre = 'Pop & Rock' WHERE id = $1")
        .bind(id)
        .execute(app.db())
        .await
        .unwrap();
    let (_, html) = page(&app, &format!("/library/albums/{id}"), &user).await;
    assert!(html.contains(r#"<span class="cell">Nena</span>"#));
    // A track without its own artist falls back to the album's.
    assert!(html.contains(r#"<span class="cell">Verschiedene Interpreten</span>"#));
    assert!(
        html.contains(r#"<a href="/library/albums?genre=Pop%20%26%20Rock">Pop &#38; Rock</a>"#)
    );

    let (status, html) = page(&app, "/library/albums?genre=pop%20%26%20rock", &user).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Bravo Hits"));
    assert!(html.contains("Genre: pop &#38; rock"));
    let (_, html) = page(&app, "/library/albums?genre=Jazz", &user).await;
    assert!(!html.contains("Bravo Hits"));
}
