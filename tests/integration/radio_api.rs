//! `GET /radio`: what it takes, what it never hands out, and that the choice follows the seed.
//! The order has chance in it, so these tests assert on sets and on rules, and where closeness
//! matters on libraries in which chance cannot outweigh it.

use std::collections::HashSet;

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, seed, seed_track};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

/// An album with `tracks` tracks of 200 s; returns its id and the track ids.
async fn album(
    app: &TestApp,
    title: &str,
    artist: Option<&str>,
    genre: Option<&str>,
    year: Option<i32>,
    tracks: i32,
) -> (i32, Vec<i32>) {
    let id = seed(
        app.db(),
        Seed {
            artist,
            ..Seed::new("album", title)
        },
    )
    .await;
    sqlx::query("UPDATE items SET genre = $2, year = $3 WHERE id = $1")
        .bind(id)
        .bind(genre)
        .bind(year)
        .execute(app.db())
        .await
        .unwrap();
    let mut ids = Vec::new();
    for position in 1..=tracks {
        let path = format!("/music/{id}/{position}.mp3");
        let track = seed_track(app.db(), id, position, &path, 200.0).await;
        sqlx::query("UPDATE tracks SET title = $2 WHERE id = $1")
            .bind(track)
            .bind(format!("{title} {position}"))
            .execute(app.db())
            .await
            .unwrap();
        ids.push(track);
    }
    (id, ids)
}

async fn radio(app: &TestApp, token: &str, query: &str) -> Vec<Value> {
    let response = app.get(&format!("/radio?{query}"), Some(token)).await;
    assert_eq!(response.status(), StatusCode::OK, "{query}");
    let body = body_json(response).await;
    body["tracks"].as_array().unwrap().clone()
}

fn ids(tracks: &[Value]) -> Vec<i32> {
    tracks
        .iter()
        .map(|t| t["id"].as_i64().unwrap() as i32)
        .collect()
}

fn item_ids(tracks: &[Value]) -> Vec<i32> {
    tracks
        .iter()
        .map(|t| t["item_id"].as_i64().unwrap() as i32)
        .collect()
}

async fn detail_of(app: &TestApp, token: &str, query: &str, status: StatusCode) -> String {
    let response = app.get(&format!("/radio?{query}"), Some(token)).await;
    assert_eq!(response.status(), status, "{query}");
    body_json(response).await["detail"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[sqlx::test]
async fn a_track_radio_hands_out_track_summaries_without_the_seed(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (first, first_tracks) = album(&app, "Eins", Some("A"), Some("Rock"), Some(1990), 2).await;
    let (second, second_tracks) = album(&app, "Zwei", Some("B"), None, None, 1).await;
    sqlx::query("UPDATE tracks SET artist = 'Gast' WHERE id = $1")
        .bind(second_tracks[0])
        .execute(app.db())
        .await
        .unwrap();

    let tracks = radio(&app, &token, &format!("track_id={}", first_tracks[0])).await;
    let got: HashSet<i32> = ids(&tracks).into_iter().collect();
    assert_eq!(got, HashSet::from([first_tracks[1], second_tracks[0]]));
    // Exactly the shape search and playlists use: the track's own artist, not the album's.
    let other = tracks
        .iter()
        .find(|t| t["id"] == json!(second_tracks[0]))
        .unwrap();
    assert_eq!(
        other,
        &json!({
            "id": second_tracks[0], "title": "Zwei 1", "artist": "Gast", "duration": 200,
            "item_id": second, "item_title": "Zwei", "item_kind": "album", "has_lyrics": false,
            "gain_db": null, "album_gain_db": null, "peak": null
        })
    );
    let same_album = tracks
        .iter()
        .find(|t| t["id"] == json!(first_tracks[1]))
        .unwrap();
    assert_eq!(same_album["artist"], Value::Null);
    assert_eq!(same_album["item_id"], json!(first));
}

#[sqlx::test]
async fn only_music_that_is_there_and_nothing_the_client_excludes(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_, seed_tracks) = album(&app, "Saat", Some("A"), Some("Rock"), None, 1).await;
    let (_, kept) = album(&app, "Da", Some("B"), Some("Rock"), None, 3).await;
    let (missing, _) = album(&app, "Weg", Some("C"), Some("Rock"), None, 2).await;
    sqlx::query("UPDATE items SET missing_since = now() WHERE id = $1")
        .bind(missing)
        .execute(app.db())
        .await
        .unwrap();
    // A copy hidden under Einstellungen → Doppelte Alben would play every song twice.
    let (copy, _) = album(&app, "Da (Kopie)", Some("B"), Some("Rock"), None, 3).await;
    sqlx::query(
        "UPDATE items SET duplicate_of = (SELECT item_id FROM tracks WHERE id = $2) WHERE id = $1",
    )
    .bind(copy)
    .bind(kept[0])
    .execute(app.db())
    .await
    .unwrap();
    let book = seed(app.db(), Seed::new("book", "Momo")).await;
    seed_track(app.db(), book, 1, "/books/momo.m4b", 3600.0).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Show")).await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge")
        },
    )
    .await;
    seed_track(app.db(), episode, 1, "/podcasts/folge.mp3", 1800.0).await;

    let query = format!("track_id={}", seed_tracks[0]);
    let got: HashSet<i32> = ids(&radio(&app, &token, &query).await)
        .into_iter()
        .collect();
    assert_eq!(got, kept.iter().copied().collect());

    let query = format!(
        "track_id={}&exclude={}&exclude={}",
        seed_tracks[0], kept[0], kept[2]
    );
    assert_eq!(ids(&radio(&app, &token, &query).await), [kept[1]]);
    // Everything excluded: an empty list is an answer, not an error.
    let query = format!("{query}&exclude={}", kept[1]);
    assert!(radio(&app, &token, &query).await.is_empty());
}

#[sqlx::test]
async fn an_album_radio_leaves_out_the_whole_album(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (seed_album, _) = album(&app, "Saat", Some("A"), Some("Rock"), Some(1990), 5).await;
    let (_, others) = album(&app, "Anderes", Some("A"), Some("Rock"), Some(1991), 2).await;
    let tracks = radio(&app, &token, &format!("item_id={seed_album}")).await;
    let got: HashSet<i32> = ids(&tracks).into_iter().collect();
    assert_eq!(got, others.iter().copied().collect());
    // Alone in the library, an album has nothing to go with.
    sqlx::query("DELETE FROM items WHERE id <> $1")
        .bind(seed_album)
        .execute(app.db())
        .await
        .unwrap();
    assert!(
        radio(&app, &token, &format!("item_id={seed_album}"))
            .await
            .is_empty()
    );
}

#[sqlx::test]
async fn an_artist_radio_plays_the_artist_and_what_is_close(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (own, _) = album(
        &app,
        "Autobahn",
        Some("Kraftwerk"),
        Some("Electronic"),
        Some(1974),
        3,
    )
    .await;
    let (near, _) = album(
        &app,
        "Neu! 75",
        Some("Neu!"),
        Some("Electronic"),
        Some(1975),
        3,
    )
    .await;
    let (far, _) = album(
        &app,
        "Kind of Blue",
        Some("Miles Davis"),
        Some("Jazz"),
        Some(1900),
        3,
    )
    .await;
    // On a sampler the artist is only a track's artist.
    let (sampler, sampler_tracks) = album(&app, "Sampler", Some("Various"), None, None, 1).await;
    sqlx::query("UPDATE tracks SET artist = 'KRAFTWERK' WHERE id = $1")
        .bind(sampler_tracks[0])
        .execute(app.db())
        .await
        .unwrap();

    // Whatever the case of the name; and with room for everything, everything comes.
    let tracks = radio(&app, &token, "artist=kraftwerk").await;
    assert_eq!(tracks.len(), 10);
    let albums: HashSet<i32> = item_ids(&tracks).into_iter().collect();
    assert_eq!(albums, HashSet::from([own, near, far, sampler]));
    // With room for three albums' first round only, the unrelated one is the one left out:
    // genre and year outweigh chance.
    for _ in 0..10 {
        let tracks = radio(&app, &token, "artist=Kraftwerk&limit=3").await;
        assert_eq!(tracks.len(), 3);
        assert!(!item_ids(&tracks).contains(&far));
    }
}

#[sqlx::test]
async fn variety_rules_hold(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_, seed_tracks) = album(&app, "Saat", Some("Saat"), Some("Rock"), Some(1990), 1).await;
    // One artist with a lot of albums, and a dozen others with one long album each.
    for n in 0..12 {
        album(
            &app,
            &format!("Viel {n}"),
            Some("Vielschreiber"),
            Some("Rock"),
            Some(1990),
            4,
        )
        .await;
    }
    for n in 0..12 {
        let artist = format!("Band {n}");
        album(
            &app,
            &format!("Einzel {n}"),
            Some(&artist),
            Some("Rock"),
            Some(1990),
            12,
        )
        .await;
    }
    let artist_of: std::collections::HashMap<i32, String> =
        sqlx::query_as("SELECT t.id, i.artist FROM tracks t JOIN items i ON i.id = t.item_id")
            .fetch_all(app.db())
            .await
            .unwrap()
            .into_iter()
            .collect();

    let mut orders = HashSet::new();
    for _ in 0..5 {
        let tracks = radio(
            &app,
            &token,
            &format!("track_id={}&limit=40", seed_tracks[0]),
        )
        .await;
        assert_eq!(tracks.len(), 40);
        let got = ids(&tracks);
        // No track twice.
        assert_eq!(got.iter().collect::<HashSet<_>>().len(), 40);
        // A few per album at most.
        let mut per_album = std::collections::HashMap::new();
        for item in item_ids(&tracks) {
            *per_album.entry(item).or_insert(0) += 1;
        }
        assert!(per_album.values().all(|count| *count <= 3), "{per_album:?}");
        // Never three in a row by one artist, and nobody takes over the list.
        let artists: Vec<&String> = got.iter().map(|id| &artist_of[id]).collect();
        assert!(
            artists.windows(3).all(|w| !(w[0] == w[1] && w[1] == w[2])),
            "{artists:?}"
        );
        let prolific = artists.iter().filter(|a| **a == "Vielschreiber").count();
        assert!(prolific <= 5, "{prolific} von 40");
        orders.insert(got);
    }
    // Chance: five calls do not give one list.
    assert!(orders.len() > 1);
}

#[sqlx::test]
async fn favourites_and_what_was_heard_a_lot_come_first_among_equals(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (_, seed_tracks) = album(&app, "Saat", Some("Saat"), None, None, 1).await;
    // No genre, no year anywhere: what is left is the listener and chance.
    let mut plain = Vec::new();
    for n in 0..40 {
        let artist = format!("Band {n}");
        plain.push(
            album(&app, &format!("Album {n}"), Some(&artist), None, None, 1)
                .await
                .0,
        );
    }
    let liked = &plain[..10];
    for (n, id) in liked.iter().enumerate() {
        if n % 2 == 0 {
            sqlx::query("INSERT INTO favorites (user_id, item_id) VALUES ($1, $2)")
                .bind(user)
                .bind(id)
                .execute(app.db())
                .await
                .unwrap();
        } else {
            sqlx::query(
                "INSERT INTO listen_events (user_id, item_id, seconds) VALUES ($1, $2, 7200)",
            )
            .bind(user)
            .bind(id)
            .execute(app.db())
            .await
            .unwrap();
        }
    }
    // A quarter of the library is liked; of the first ten tracks clearly more than a quarter
    // are, summed over some calls so that chance evens out.
    let mut from_liked = 0;
    for _ in 0..20 {
        let tracks = radio(
            &app,
            &token,
            &format!("track_id={}&limit=10", seed_tracks[0]),
        )
        .await;
        assert_eq!(tracks.len(), 10);
        from_liked += item_ids(&tracks)
            .iter()
            .filter(|id| liked.contains(id))
            .count();
    }
    assert!(from_liked > 80, "{from_liked} von 200");
}

#[sqlx::test]
async fn the_seed_has_to_be_one_known_and_music(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (music, tracks) = album(&app, "Musik", Some("A"), None, None, 1).await;
    let book = seed(app.db(), Seed::new("book", "Momo")).await;
    let chapter = seed_track(app.db(), book, 1, "/books/momo.m4b", 3600.0).await;

    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    let one = "Genau einer der Parameter track_id, item_id oder artist muss angegeben sein";
    for query in [
        String::new(),
        "limit=5".to_owned(),
        "artist=".to_owned(),
        format!("track_id={}&item_id={music}", tracks[0]),
        format!("item_id={music}&artist=A"),
    ] {
        assert_eq!(detail_of(&app, &token, &query, unprocessable).await, one);
    }
    assert_eq!(
        detail_of(&app, &token, "track_id=abc", unprocessable).await,
        "track_id muss eine ganze Zahl sein"
    );
    assert_eq!(
        detail_of(&app, &token, "item_id=1.5", unprocessable).await,
        "item_id muss eine ganze Zahl sein"
    );
    for query in [format!("item_id={book}"), format!("track_id={chapter}")] {
        assert_eq!(
            detail_of(&app, &token, &query, unprocessable).await,
            "Titel-Radio gibt es nur für Musik"
        );
    }
    let not_found = StatusCode::NOT_FOUND;
    assert_eq!(
        detail_of(&app, &token, "track_id=999999", not_found).await,
        "Unbekannter Track"
    );
    assert_eq!(
        detail_of(&app, &token, "item_id=999999", not_found).await,
        "Unbekanntes Item"
    );
    assert_eq!(
        detail_of(&app, &token, "artist=Niemand", not_found).await,
        "Interpret nicht gefunden"
    );
    let response = app.get(&format!("/radio?item_id={music}"), None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn limit_and_exclude_are_checked(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (music, _) = album(&app, "Musik", Some("A"), None, None, 1).await;
    for n in 0..8 {
        album(
            &app,
            &format!("Mehr {n}"),
            Some(&format!("B{n}")),
            None,
            None,
            2,
        )
        .await;
    }
    let seed = format!("item_id={music}");
    assert_eq!(
        radio(&app, &token, &format!("{seed}&limit=4")).await.len(),
        4
    );
    // Short of the default, the library gives what it has: 8 albums of 2.
    assert_eq!(radio(&app, &token, &seed).await.len(), 16);

    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    for bad in ["0", "201", "x"] {
        assert_eq!(
            detail_of(&app, &token, &format!("{seed}&limit={bad}"), unprocessable).await,
            "limit außerhalb des gültigen Bereichs"
        );
    }
    assert_eq!(
        detail_of(&app, &token, &format!("{seed}&exclude=x"), unprocessable).await,
        "exclude muss eine ganze Zahl sein"
    );
    let many = |count: usize| {
        let excluded: Vec<String> = (0..count)
            .map(|n| format!("exclude={}", n + 100_000))
            .collect();
        format!("{seed}&{}", excluded.join("&"))
    };
    assert_eq!(radio(&app, &token, &many(500)).await.len(), 16);
    assert_eq!(
        detail_of(&app, &token, &many(501), unprocessable).await,
        "Höchstens 500 exclude-Werte je Anfrage"
    );
}
