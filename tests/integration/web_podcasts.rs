//! Podcast pages and forms at the HTTP level (legacy library/podcasts/**, settings/podcasts and the
//! podcasts e2e spec where it asserts on markup).

use std::path::Path;

use axum::http::{StatusCode, header};
use chrono::{Duration, Utc};
use sqlx::PgPool;

use autoreverse_server::Config;

use crate::common::{
    FakeWeb, Seed, TestApp, body_text, header_str, seed, seed_progress, seed_track,
};

const FEED: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Maschinenraum</title>
<description>Technik &amp; mehr</description>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
<item><title>Folge 117</title><guid>ep-117</guid><pubDate>Thu, 25 Dec 2025 10:00:00 GMT</pubDate><enclosure url="https://x/117.mp3"/></item>
</channel></rss>"#;

fn app_with(db: PgPool, data: &Path, refresh_hours: &str) -> TestApp {
    let data = data.to_str().unwrap().to_owned();
    let hours = refresh_hours.to_owned();
    let config = Config::from_lookup(|key| match key {
        "AUTOREVERSE_DATA" => Some(data.clone()),
        "AUTOREVERSE_PODCAST_REFRESH_HOURS" => Some(hours.clone()),
        _ => None,
    })
    .unwrap();
    TestApp::with_config(db, config)
}

async fn cookie(app: &TestApp, name: &str, admin: bool) -> (String, i32) {
    let id = app.user(name, admin).await;
    let token = app.token(id).await;
    (format!("autoreverse_session={token}"), id)
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
    let response = app.get_with_cookie(uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

/// A podcast subscribed a week ago with two episodes: one new since then, one older.
async fn show(app: &TestApp) -> (i32, i32, i32) {
    let podcast = seed(
        app.db(),
        Seed {
            added_at: Some(Utc::now() - Duration::days(7)),
            ..Seed::new("podcast", "Maschinenraum")
        },
    )
    .await;
    let mut ids = Vec::new();
    for (title, days_ago) in [("Neue Folge", 1), ("Alte Folge", 30)] {
        let id: i32 = sqlx::query_scalar(
            "INSERT INTO items (kind, parent_id, title, sort_title, guid, published_at, feed_duration)
             VALUES ('episode', $1, $2, lower($2), $2, $3, 3600) RETURNING id",
        )
        .bind(podcast)
        .bind(title)
        .bind(Utc::now() - Duration::days(days_ago))
        .fetch_one(app.db())
        .await
        .unwrap();
        ids.push(id);
    }
    (podcast, ids[0], ids[1])
}

#[sqlx::test]
async fn the_pages_need_a_session_and_the_admin_parts_a_verwalter(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/library/podcasts",
        "/library/podcasts/1",
        "/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml",
        "/settings/podcasts",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
    cookie(&app, "oliver", true).await;
    let (user, _) = cookie(&app, "gast", false).await;
    for uri in [
        "/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml",
        "/settings/podcasts",
    ] {
        let response = app.get_with_cookie(uri, &user).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/library", "{uri}");
    }
    let (podcast, _, _) = show(&app).await;
    for uri in [
        "/library/podcasts/subscribe".to_owned(),
        format!("/library/podcasts/{podcast}/keep"),
        format!("/library/podcasts/{podcast}/refresh"),
        format!("/library/podcasts/{podcast}/unsubscribe"),
        "/settings/podcasts".to_owned(),
    ] {
        let response = app.post_form(&uri, "keep=3&feed_url=x", Some(&user)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/library", "{uri}");
    }
}

#[sqlx::test]
async fn the_overview_shows_the_rail_and_the_search_for_a_verwalter(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "oliver", true).await;
    let empty = page(&app, "/library/podcasts", &admin).await;
    assert!(empty.contains("Abos · 0"));
    assert!(empty.contains("Noch keine Podcast-Abos."));

    let (podcast, _, _) = show(&app).await;
    let html = page(&app, "/library/podcasts?q=Tennis", &admin).await;
    assert!(html.contains("Abos · 1"));
    assert!(html.contains(&format!("href=\"/library/podcasts/{podcast}\"")));
    assert!(html.contains("2 Folgen · 1 neu"), "{html}");
    assert!(html.contains("<span class=\"badge\">1</span>"));
    assert!(html.contains("Wähle einen Podcast aus der Liste."));
    assert!(html.contains("placeholder=\"Podcast suchen\""));
    assert!(
        html.contains("value=\"Tennis\""),
        "the search is seeded from ?q="
    );

    let (user, _) = cookie(&app, "gast", false).await;
    let html = page(&app, "/library/podcasts", &user).await;
    assert!(!html.contains("Podcast suchen"));
}

#[sqlx::test]
async fn new_episodes_lists_what_came_since_the_subscription(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, user) = cookie(&app, "oliver", true).await;
    let (podcast, new, old) = show(&app).await;
    let html = page(&app, "/library/podcasts?filter=new", &admin).await;
    assert!(html.contains("<h1>Neue Folgen</h1>"));
    assert!(html.contains("1 Folge aus 1 Abo"));
    assert!(html.contains("Neue Folge"));
    assert!(!html.contains("Alte Folge"));
    assert!(html.contains(&format!(
        "<a class=\"show\" href=\"/library/podcasts/{podcast}\">Maschinenraum</a>"
    )));
    assert!(html.contains("Gestern"));
    assert!(html.contains("1 Std"), "the feed's duration stands in");
    assert!(html.contains(&format!(
        "data-action=\"play-episode\" data-episode-id=\"{new}\""
    )));
    assert!(html.contains("aria-label=\"Folge herunterladen\""));

    seed_progress(app.db(), user, new, 3600.0, true).await;
    let html = page(&app, "/library/podcasts?filter=new", &admin).await;
    assert!(html.contains("Nichts Ungehörtes"));
    assert!(html.contains("Alles gehört"));
    let _ = old;
}

#[sqlx::test]
async fn the_detail_page_lists_numbered_episodes_with_filters_and_progress(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, user) = cookie(&app, "oliver", true).await;
    let (podcast, new, old) = show(&app).await;
    seed_track(app.db(), old, 1, "/p/alt.mp3", 1800.0).await;
    seed_progress(app.db(), user, new, 1800.0, false).await;
    let html = page(&app, &format!("/library/podcasts/{podcast}"), &admin).await;
    assert!(html.contains("<h1>Maschinenraum</h1>"));
    assert!(html.contains("2 Folgen"));
    assert!(html.contains("1 ungehört"));
    assert!(html.contains("zuletzt geprüft nie"));
    // Newest first, the oldest is number one.
    let new_at = html.find("Neue Folge").unwrap();
    let old_at = html.find("Alte Folge").unwrap();
    assert!(new_at < old_at);
    assert!(html.contains("<span class=\"index mono\" aria-hidden=\"true\">2</span>"));
    for pill in ["Alle", "Ungehört", "Angefangen", "Geladen"] {
        assert!(html.contains("data-episode-filter=") && html.contains(pill));
    }
    assert!(html.contains("Neue Folge fortsetzen"));
    assert!(html.contains("noch 30 min"));
    assert!(html.contains("width: 50%"));
    assert!(html.contains("Fortsetzen</button>") || html.contains("Fortsetzen"));
    assert!(html.contains(&format!("data-episode-id=\"{new}\"")));
    assert!(html.contains("data-status=\"started\""));
    assert!(html.contains("data-downloaded=\"true\""));
    assert!(html.contains("Folgen vorhalten"));
    assert!(html.contains("Feed-URL hinzufügen"));
    assert!(html.contains("Abo kündigen"));

    let (guest, _) = cookie(&app, "gast", false).await;
    let html = page(&app, &format!("/library/podcasts/{podcast}"), &guest).await;
    assert!(!html.contains("Folgen vorhalten"));
    assert!(!html.contains("Abo kündigen"));
    assert!(html.contains("Neueste abspielen"));
}

#[sqlx::test]
async fn an_episode_id_leads_to_its_podcast_and_anything_else_is_404(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "oliver", true).await;
    let (podcast, new, _) = show(&app).await;
    let response = app
        .get_with_cookie(&format!("/library/podcasts/{new}"), &admin)
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, header::LOCATION),
        format!("/library/podcasts/{podcast}")
    );
    let album = seed(app.db(), Seed::new("album", "A")).await;
    for uri in [
        format!("/library/podcasts/{album}"),
        "/library/podcasts/abc".into(),
    ] {
        let response = app.get_with_cookie(&uri, &admin).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert!(body_text(response).await.contains("Unbekannter Podcast"));
    }
}

#[sqlx::test]
async fn keep_sets_clears_and_refuses_out_of_range(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "oliver", true).await;
    let (podcast, _, _) = show(&app).await;
    let uri = format!("/library/podcasts/{podcast}/keep");
    let keep = || async {
        sqlx::query_scalar::<_, Option<i32>>("SELECT keep_episodes FROM items WHERE id = $1")
            .bind(podcast)
            .fetch_one(app.db())
            .await
            .unwrap()
    };
    let response = app.post_form(&uri, "keep=3", Some(&admin)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(keep().await, Some(3));
    let response = app.post_form(&uri, "keep=99", Some(&admin)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_text(response)
            .await
            .contains("Bitte eine Zahl zwischen 0 und 50")
    );
    assert_eq!(keep().await, Some(3));
    app.post_form(&uri, "keep=", Some(&admin)).await;
    assert_eq!(keep().await, None);
}

#[sqlx::test]
async fn subscribe_refresh_and_unsubscribe_through_the_forms(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), "0");
    let (admin, _) = cookie(&app, "oliver", true).await;
    let form = format!(
        "feed_url={}",
        autoreverse_server::web::format::encode_component(&web.url("/feed.xml"))
    );
    let response = app
        .post_form("/library/podcasts/subscribe", &form, Some(&admin))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = header_str(&response, header::LOCATION).to_owned();
    let id: i32 = location.rsplit('/').next().unwrap().parse().unwrap();
    let html = page(&app, &location, &admin).await;
    assert!(html.contains("<h1>Maschinenraum</h1>"));
    assert!(html.contains("zuletzt geprüft heute"));

    let refresh = format!("/library/podcasts/{id}/refresh");
    let response = app.post_form(&refresh, "", Some(&admin)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    web.set("/feed.xml", 200, "<html>kaputt</html>");
    let response = app.post_form(&refresh, "", Some(&admin)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_text(response)
            .await
            .contains("Der Feed konnte nicht gelesen werden.")
    );

    let response = app
        .post_form(
            &format!("/library/podcasts/{id}/unsubscribe"),
            "",
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library/podcasts");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(left, 0);

    let response = app
        .post_form(
            "/library/podcasts/subscribe",
            "feed_url=http%3A%2F%2F127.0.0.1%3A9%2Fx",
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_text(response)
            .await
            .contains("Der Feed ist nicht erreichbar.")
    );
}

#[sqlx::test]
async fn the_preview_page_shows_the_feed_and_subscribes(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), "0");
    let (admin, _) = cookie(&app, "oliver", true).await;
    let feed = autoreverse_server::web::format::encode_component(&web.url("/feed.xml"));
    let uri = format!("/library/podcasts/preview?feed={feed}&q=Technik");
    let html = page(&app, &uri, &admin).await;
    assert!(html.contains("<h1>Maschinenraum</h1>"));
    assert!(html.contains("2 Folgen im Feed"));
    assert!(html.contains("Technik &#38; mehr"), "{html}");
    assert!(html.contains("01.01.2026"));
    assert!(html.contains("href=\"/library/podcasts?q=Technik\""));
    assert!(html.contains(">Abonnieren</button>"));

    let response = app.post_form(&uri, "", Some(&admin)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = header_str(&response, header::LOCATION).to_owned();
    assert!(location.starts_with("/library/podcasts/"));
    let html = page(&app, &uri, &admin).await;
    assert!(html.contains("Bereits abonniert"));
    assert!(html.contains(&format!("href=\"{location}\"")));
}

#[sqlx::test]
async fn the_preview_page_reports_an_unreachable_or_missing_feed(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "oliver", true).await;
    let html = page(
        &app,
        "/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml&q=Tennis",
        &admin,
    )
    .await;
    assert!(html.contains("Der Feed ist nicht erreichbar."));
    assert!(
        html.contains(
            "<a class=\"back\" href=\"/library/podcasts?q=Tennis\">← Zurück zur Suche</a>"
        )
    );
    for (uri, message) in [
        ("/library/podcasts/preview", "Keine Feed-Adresse angegeben"),
        (
            "/library/podcasts/preview?feed=ftp%3A%2F%2Fx",
            "Ungültige Feed-Adresse",
        ),
    ] {
        let response = app.get_with_cookie(uri, &admin).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        assert!(body_text(response).await.contains(message));
    }
}

#[sqlx::test]
async fn settings_podcasts_saves_the_default_and_explains_the_refresh(db: PgPool) {
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), "1");
    let (admin, _) = cookie(&app, "oliver", true).await;
    let html = page(&app, "/settings/podcasts", &admin).await;
    assert!(html.contains("<h1>Podcast-Abos</h1>"));
    assert!(html.contains("jede Stunde von selbst"));
    assert!(html.contains("placeholder=\"Podcast suchen\""));
    let response = app
        .post_form("/settings/podcasts", "keep=4", Some(&admin))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Gespeichert"));
    assert!(html.contains("value=\"4\""));
    let response = app
        .post_form("/settings/podcasts", "keep=1.5", Some(&admin))
        .await;
    assert!(
        body_text(response)
            .await
            .contains("Bitte eine Zahl zwischen 0 und 50")
    );

    let off = app_with(app.db().clone(), data.path(), "0");
    let html = page(&off, "/settings/podcasts", &admin).await;
    assert!(html.contains("AUTOREVERSE_PODCAST_REFRESH_HOURS=0"));
}

#[sqlx::test]
async fn episodes_go_into_playlists_from_both_podcast_pages(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(db, dir.path(), "0");
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let (podcast, newer, _older) = show(&app).await;
    let playlist: i32 = sqlx::query_scalar(
        "INSERT INTO playlists (user_id, name) VALUES ($1, 'Unterwegs') RETURNING id",
    )
    .bind(user)
    .fetch_one(app.db())
    .await
    .unwrap();

    for uri in [
        format!("/library/podcasts/{podcast}"),
        "/library/podcasts?filter=new".to_owned(),
    ] {
        let html = page(&app, &uri, &cookie).await;
        assert!(html.contains("data-playlist-picker"), "{uri}");
        assert!(
            html.contains(&format!(r#"data-playlist-target="{playlist}""#)),
            "{uri}"
        );
        assert!(
            html.contains(&format!(
                r#"data-playlist-pick data-pick-item="{newer}" aria-label="Neue Folge zur Playlist hinzufügen""#
            )),
            "{uri}"
        );
    }

    // What the dialog sends; the playlist then lists the episode under its podcast, with the
    // length its feed claims (not downloaded yet), ready for the queue to download and play.
    sqlx::query("INSERT INTO playlist_entries (playlist_id, item_id, position) VALUES ($1, $2, 1)")
        .bind(playlist)
        .bind(newer)
        .execute(app.db())
        .await
        .unwrap();
    let html = page(&app, &format!("/library/playlists/{playlist}"), &cookie).await;
    assert!(html.contains(&format!(
        r#"data-entry-item="{newer}" data-entry-kind="episode""#
    )));
    assert!(html.contains(r#"<span class="cell">Maschinenraum</span>"#));
    assert!(html.contains(r#"<span class="cell mono">60:00</span>"#));
}
