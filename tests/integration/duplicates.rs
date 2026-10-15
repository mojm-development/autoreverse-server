//! Einstellungen → Doppelte Alben: finding copies, keeping one, and the copies leaving the lists.

use autoreverse_server::library::duplicates::{groups, keep, keep_all_recommended, unhide};
use axum::http::{StatusCode, header};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, body_text, header_str, seed, seed_track};

fn album<'a>(title: &'a str, artist: &'a str, path: &'a str) -> Seed<'a> {
    Seed {
        artist: Some(artist),
        source_path: Some(path),
        ..Seed::new("album", title)
    }
}

async fn with_tracks(db: &PgPool, seed_: Seed<'_>, count: i32) -> i32 {
    let path = seed_.source_path.unwrap().to_owned();
    let id = seed(db, seed_).await;
    for n in 1..=count {
        seed_track(db, id, n, &format!("{path}/{n}.mp3"), 200.0).await;
    }
    id
}

async fn cookie(app: &TestApp, is_admin: bool) -> String {
    let id = app.user("oliver", is_admin).await;
    format!("autoreverse_session={}", app.token(id).await)
}

#[sqlx::test]
async fn groups_same_artist_and_title_ignoring_case_and_spaces_but_not_editions(db: PgPool) {
    let short = with_tracks(&db, album("Aggro Berlin", "Sido", "/m/a"), 10).await;
    let full = with_tracks(&db, album("aggro  berlin ", "SIDO", "/m/b"), 12).await;
    with_tracks(&db, album("Aggro Berlin (Deluxe)", "Sido", "/m/c"), 14).await;
    with_tracks(&db, album("Aggro Berlin", "Bushido", "/m/d"), 9).await;

    let found = groups(&db).await.unwrap();
    assert_eq!(found.len(), 1);
    let ids: Vec<i32> = found[0].copies.iter().map(|c| c.id).collect();
    assert_eq!(
        ids,
        [full, short],
        "the copy with more tracks is recommended first"
    );
    assert!(found[0].is_open());
}

#[sqlx::test]
async fn keeping_one_hides_the_others_from_the_lists_until_shown_again(db: PgPool) {
    let app = TestApp::new(db.clone());
    let kept = with_tracks(&db, album("7", "David Guetta", "/m/a"), 15).await;
    let copy = with_tracks(&db, album("7", "David Guetta", "/m/b"), 15).await;
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    let titles = |body: serde_json::Value| -> Vec<i64> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_i64().unwrap())
            .collect()
    };

    assert_eq!(keep(&db, kept).await.unwrap(), Some(1));
    let listed = body_json(app.get("/items?kind=album", Some(&token)).await).await;
    assert_eq!(titles(listed), [i64::from(kept)]);
    assert!(!groups(&db).await.unwrap()[0].is_open());
    // Still reachable by id: playlists and progress may point at it.
    let direct = app.get(&format!("/items/{copy}"), Some(&token)).await;
    assert_eq!(direct.status(), StatusCode::OK);

    assert!(unhide(&db, copy).await.unwrap());
    let listed = body_json(app.get("/items?kind=album", Some(&token)).await).await;
    assert_eq!(titles(listed).len(), 2);
}

#[sqlx::test]
async fn a_hidden_copy_returns_when_the_kept_album_is_gone(db: PgPool) {
    let kept = with_tracks(&db, album("13", "Die Ärzte", "/m/a"), 10).await;
    let copy = with_tracks(&db, album("13", "Die Ärzte", "/m/b"), 10).await;
    keep(&db, kept).await.unwrap();
    sqlx::query("DELETE FROM items WHERE id = $1")
        .bind(kept)
        .execute(&db)
        .await
        .unwrap();
    let hidden: Option<i32> = sqlx::query_scalar("SELECT duplicate_of FROM items WHERE id = $1")
        .bind(copy)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(hidden, None);
}

#[sqlx::test]
async fn keep_all_takes_every_recommendation_and_keep_refuses_a_non_album(db: PgPool) {
    with_tracks(&db, album("19", "Adele", "/m/a"), 11).await;
    let best = with_tracks(&db, album("19", "Adele", "/m/b"), 12).await;
    with_tracks(&db, album("2020", "Bon Jovi", "/m/c"), 10).await;
    with_tracks(&db, album("2020", "Bon Jovi", "/m/d"), 10).await;
    let book = seed(&db, Seed::new("book", "Der Schwarm")).await;

    assert_eq!(keep_all_recommended(&db).await.unwrap(), 2);
    let found = groups(&db).await.unwrap();
    assert!(found.iter().all(|g| !g.is_open()));
    let adele = found
        .iter()
        .find(|g| g.recommended().title == "19")
        .unwrap();
    assert_eq!(adele.recommended().id, best);
    assert_eq!(keep(&db, book).await.unwrap(), None);
}

#[sqlx::test]
async fn the_page_lists_open_groups_for_verwalter_only_and_keeps_by_form(db: PgPool) {
    let app = TestApp::new(db.clone());
    let first = with_tracks(&db, album("1989", "Taylor Swift", "/m/a"), 13).await;
    let second = with_tracks(&db, album("1989", "Taylor Swift", "/m/b"), 10).await;

    // The first account is a Verwalter whatever it asks for, so the admin comes first.
    let id = app.user("admin", true).await;
    let admin = format!("autoreverse_session={}", app.token(id).await);
    let user = cookie(&app, false).await;
    let refused = app.get_with_cookie("/settings/duplicates", &user).await;
    assert_eq!(refused.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&refused, header::LOCATION), "/library");
    let html = body_text(app.get_with_cookie("/settings/duplicates", &admin).await).await;
    assert!(html.contains("<h1>Doppelte Alben</h1>"));
    assert!(html.contains("1 Gruppe offen"));
    assert!(html.contains(&format!(r#"id="gruppe-{first}""#)));
    assert!(html.contains("13 Titel"));
    assert!(html.contains("Empfohlen"));

    let response = app
        .post_form(
            "/settings/duplicates",
            &format!("action=keep&id={second}&next="),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, header::LOCATION),
        "/settings/duplicates"
    );
    let html = body_text(app.get_with_cookie("/settings/duplicates", &admin).await).await;
    assert!(html.contains("Keine doppelten Alben offen."));
    assert!(html.contains("Wieder einblenden"));
    assert!(html.contains("stattdessen: 1989 (/m/b)"));
}
