//! Item queries and sort orders (legacy tests/integration/queries-core.test.ts, sorting.test.ts
//! and the book parts of queries-aggregate.test.ts).

use autoreverse_server::library::queries::{
    ItemsFilter, SortKey, book_series, count_items, count_missing, delete_missing, item_durations,
    items, library_counts, progress_map, series_siblings, tracks,
};
use autoreverse_server::library::sorting::{episode_number, natural_key};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, seed, seed_progress, seed_track};

fn filter(kind: Option<&str>, sort: SortKey) -> ItemsFilter {
    ItemsFilter {
        kind: kind.map(Into::into),
        sort: Some(sort),
        limit: 50,
        offset: 0,
        ..Default::default()
    }
}

async fn titles(db: &PgPool, filter: &ItemsFilter) -> Vec<String> {
    items(db, filter)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

async fn seed_all(db: &PgPool, kind: &str, list: &[&str], template: Seed<'_>) {
    for title in list {
        seed(
            db,
            Seed {
                kind,
                title,
                ..template.clone()
            },
        )
        .await;
    }
}

#[sqlx::test]
async fn items_filters_by_kind_searches_case_insensitively_and_paginates(db: PgPool) {
    seed_all(
        &db,
        "album",
        &["Nordlicht", "Halbschatten"],
        Seed::default(),
    )
    .await;
    seed(&db, Seed::new("book", "Eine Billion Dollar")).await;
    assert_eq!(
        titles(&db, &filter(Some("album"), SortKey::Title)).await,
        ["Halbschatten", "Nordlicht"]
    );
    let search = ItemsFilter {
        q: Some("BILLION".into()),
        ..filter(None, SortKey::Title)
    };
    assert_eq!(titles(&db, &search).await, ["Eine Billion Dollar"]);
    let page = ItemsFilter {
        limit: 1,
        offset: 1,
        ..filter(Some("album"), SortKey::Title)
    };
    assert_eq!(titles(&db, &page).await, ["Nordlicht"]);
}

#[sqlx::test]
async fn search_treats_like_wildcards_literally(db: PgPool) {
    seed_all(
        &db,
        "book",
        &["100% Hörspiel", "1000 Hörspiele"],
        Seed::default(),
    )
    .await;
    let search = ItemsFilter {
        q: Some("0%".into()),
        ..filter(None, SortKey::Title)
    };
    assert_eq!(titles(&db, &search).await, ["100% Hörspiel"]);
}

#[sqlx::test]
async fn items_leaves_out_children(db: PgPool) {
    let podcast = seed(&db, Seed::new("podcast", "Maschinenraum")).await;
    seed(
        &db,
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge 1")
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(None, SortKey::Title)).await,
        ["Maschinenraum"]
    );
}

#[sqlx::test]
async fn missing_filters_on_missing_since(db: PgPool) {
    seed(
        &db,
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    seed(&db, Seed::new("album", "Da")).await;
    let missing = |m| ItemsFilter {
        missing: Some(m),
        ..filter(None, SortKey::Title)
    };
    assert_eq!(titles(&db, &missing(true)).await, ["Weg"]);
    assert_eq!(titles(&db, &missing(false)).await, ["Da"]);
    assert_eq!(count_items(&db, "album", None).await.unwrap(), 1);
    assert_eq!(library_counts(&db).await.unwrap().album_count, 1);
    assert_eq!(count_missing(&db, Some("album")).await.unwrap(), 1);
    assert_eq!(count_missing(&db, Some("book")).await.unwrap(), 0);
}

#[sqlx::test]
async fn delete_missing_removes_only_missing_rows(db: PgPool) {
    seed(
        &db,
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    seed(&db, Seed::new("album", "Da")).await;
    assert_eq!(delete_missing(&db).await.unwrap(), 1);
    assert_eq!(count_items(&db, "album", None).await.unwrap(), 1);
}

#[sqlx::test]
async fn tracks_come_in_position_order(db: PgPool) {
    let album = seed(&db, Seed::new("album", "X")).await;
    seed_track(&db, album, 2, "/b", 10.0).await;
    seed_track(&db, album, 1, "/a", 5.0).await;
    let positions: Vec<i32> = tracks(&db, album)
        .await
        .unwrap()
        .iter()
        .map(|t| t.position)
        .collect();
    assert_eq!(positions, [1, 2]);
}

#[sqlx::test]
async fn natural_sort_orders_embedded_numbers_numerically(db: PgPool) {
    // A TKKG library: plain lower(sort_title) puts '100/' between '10/' and '11/'.
    seed_all(
        &db,
        "album",
        &[
            "11/Elf",
            "2/Zwei",
            "100/Hundert",
            "10/Zehn",
            "1/Eins",
            "20/Zwanzig",
        ],
        Seed::default(),
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("album"), SortKey::Title)).await,
        [
            "1/Eins",
            "2/Zwei",
            "10/Zehn",
            "11/Elf",
            "20/Zwanzig",
            "100/Hundert"
        ]
    );
}

#[sqlx::test]
async fn natural_sort_stays_alphabetical_without_numbers(db: PgPool) {
    seed_all(
        &db,
        "album",
        &["Zenit", "Halbschatten", "nordlicht"],
        Seed::default(),
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("album"), SortKey::Title)).await,
        ["Halbschatten", "nordlicht", "Zenit"]
    );
}

#[sqlx::test]
async fn natural_key_is_defined_for_digits_only_and_empty_titles(db: PgPool) {
    let sql = format!(
        "SELECT {} FROM (VALUES ('7'), ('')) AS t(title) ORDER BY 1",
        natural_key("t.title")
    );
    let keys: Vec<Vec<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(keys, [vec![], vec!["000000000007".to_owned()]]);
}

async fn number_for(db: &PgPool, title: &str) -> Option<i32> {
    let sql = format!("SELECT {}", episode_number("$1::text"));
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(title)
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn episode_number_reads_labelled_and_leading_numbers(db: PgPool) {
    for (title, n) in [
        ("Die neuen Fälle, Fall 04: Der Fluch", 4),
        ("Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch", 4),
        ("Folge 12 – Das Erbe", 12),
        ("Teil 3", 3),
        ("Nr. 5 Der Hafen", 5),
        ("Vol. 2", 2),
        ("04/Der Fluch", 4),
        ("117 - Im Schloß der schlafenden Vampire", 117),
        ("08. Das Phantom", 8),
    ] {
        assert_eq!(number_for(&db, title).await, Some(n), "{title}");
    }
}

#[sqlx::test]
async fn episode_number_does_not_invent_numbers(db: PgPool) {
    for title in [
        "1984",
        "Nordlicht",
        "Ein Zufall 3",
        "Der Anteil 2",
        "Volumen 5",
        "100,000,000 Bon Jovi Fans Can't Be Wrong",
        "1.000 Meilen unter dem Meer",
        "100% Ginuwine",
    ] {
        assert_eq!(number_for(&db, title).await, None, "{title}");
    }
}

#[sqlx::test]
async fn series_sort_runs_a_series_in_instalment_order(db: PgPool) {
    seed_all(
        &db,
        "book",
        &[
            "Die neuen Fälle, Fall 01: Besuche eines Gentlemans",
            "Die neuen Fälle, Fall 30: Das Rätsel der Aurora",
            "Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch",
            "Sherlock Holmes - Die neuen Fälle, Fall 05: Die Bestie",
            "Die neuen Fälle, Fall 02: Die Gesellschaft der Verlorenen",
        ],
        Seed {
            author: Some("Sherlock Holmes"),
            ..Seed::default()
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("book"), SortKey::Series)).await,
        [
            "Die neuen Fälle, Fall 01: Besuche eines Gentlemans",
            "Die neuen Fälle, Fall 02: Die Gesellschaft der Verlorenen",
            "Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch",
            "Sherlock Holmes - Die neuen Fälle, Fall 05: Die Bestie",
            "Die neuen Fälle, Fall 30: Das Rätsel der Aurora",
        ]
    );
}

#[sqlx::test]
async fn series_sort_groups_by_author_before_instalment(db: PgPool) {
    seed(
        &db,
        Seed {
            author: Some("Alpha"),
            ..Seed::new("book", "Fall 02: B")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            author: Some("Beta"),
            ..Seed::new("book", "Fall 01: A")
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("book"), SortKey::Series)).await,
        ["Fall 02: B", "Fall 01: A"]
    );
}

#[sqlx::test]
async fn series_sort_puts_unnumbered_titles_last_alphabetically(db: PgPool) {
    seed_all(
        &db,
        "book",
        &["Fall 02: B", "Zwischenspiel", "Fall 01: A", "Anhang"],
        Seed {
            author: Some("Alpha"),
            ..Seed::default()
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("book"), SortKey::Series)).await,
        ["Fall 01: A", "Fall 02: B", "Anhang", "Zwischenspiel"]
    );
}

#[sqlx::test]
async fn series_sort_prefers_the_stored_volume_number(db: PgPool) {
    seed(
        &db,
        Seed {
            series: Some("S"),
            series_index: Some(2.0),
            ..Seed::new("book", "Fall 1: Später")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            series: Some("S"),
            series_index: Some(1.0),
            ..Seed::new("book", "Fall 9: Zuerst")
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(Some("book"), SortKey::Series)).await,
        ["Fall 9: Zuerst", "Fall 1: Später"]
    );
}

#[sqlx::test]
async fn added_sort_puts_the_newest_first(db: PgPool) {
    let at = |s: &str| Some(s.parse::<chrono::DateTime<chrono::Utc>>().unwrap());
    seed(
        &db,
        Seed {
            added_at: at("2020-01-01T00:00:00Z"),
            ..Seed::new("album", "Alpha")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            added_at: at("2026-01-01T00:00:00Z"),
            ..Seed::new("album", "Zeta")
        },
    )
    .await;
    assert_eq!(
        titles(&db, &filter(None, SortKey::Added)).await,
        ["Zeta", "Alpha"]
    );
}

#[sqlx::test]
async fn played_sort_orders_by_the_asking_users_progress(db: PgPool) {
    let app = TestApp::new(db.clone());
    let (mine, theirs) = (
        app.user("oliver", false).await,
        app.user("someone", false).await,
    );
    let old = seed(&db, Seed::new("album", "Alt")).await;
    let new = seed(&db, Seed::new("album", "Neu")).await;
    seed(&db, Seed::new("album", "Nie gehört")).await;
    let other = seed(&db, Seed::new("album", "Ihres")).await;
    seed_progress(&db, mine, old, 10.0, false).await;
    seed_progress(&db, mine, new, 20.0, false).await;
    seed_progress(&db, theirs, other, 5.0, false).await;
    sqlx::query("UPDATE progress SET updated_at = now() - interval '1 day' WHERE item_id = $1")
        .bind(old)
        .execute(&db)
        .await
        .unwrap();
    let played = ItemsFilter {
        played_by: Some(mine),
        ..filter(None, SortKey::Played)
    };
    assert_eq!(
        titles(&db, &played).await,
        ["Neu", "Alt", "Ihres", "Nie gehört"]
    );
}

#[sqlx::test]
async fn series_siblings_order_by_instalment_and_scope_by_author(db: PgPool) {
    seed_all(
        &db,
        "book",
        &["Fall 10: Zehn", "Fall 2: Zwei", "Fall 1: Eins"],
        Seed {
            series: Some("Die neuen Fälle"),
            ..Seed::default()
        },
    )
    .await;
    let rows = series_siblings(&db, "Die neuen Fälle", None).await.unwrap();
    let titles: Vec<_> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["Fall 1: Eins", "Fall 2: Zwei", "Fall 10: Zehn"]);

    seed(
        &db,
        Seed {
            series: Some("Sternenwind"),
            author: Some("Erste"),
            ..Seed::new("book", "Ihr Band")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            series: Some("Sternenwind"),
            author: Some("Zweite"),
            ..Seed::new("book", "Sein Band")
        },
    )
    .await;
    let scoped = series_siblings(&db, "Sternenwind", Some("Erste"))
        .await
        .unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].title, "Ihr Band");
    assert_eq!(
        series_siblings(&db, "Sternenwind", None)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test]
async fn progress_map_and_durations_batch_many_items(db: PgPool) {
    let app = TestApp::new(db.clone());
    let user = app.user("oliver", false).await;
    let a = seed(&db, Seed::new("book", "A")).await;
    let b = seed(&db, Seed::new("book", "B")).await;
    seed_track(&db, a, 1, "/a1", 100.0).await;
    seed_track(&db, a, 2, "/a2", 50.0).await;
    seed_progress(&db, user, a, 10.0, false).await;
    seed_progress(&db, user, b, 20.0, true).await;

    let map = progress_map(&db, user, &[a, b]).await.unwrap();
    assert_eq!((map[&a].position, map[&a].duration), (10.0, 150.0));
    assert!(map[&b].finished);
    assert!(progress_map(&db, user, &[]).await.unwrap().is_empty());

    let durations = item_durations(&db, &[a, b]).await.unwrap();
    assert_eq!(durations.get(&a), Some(&150.0));
    assert!(!durations.contains_key(&b));
    assert!(item_durations(&db, &[]).await.unwrap().is_empty());
}

#[sqlx::test]
async fn book_series_groups_a_shelf_per_author_with_covers_and_finished_volumes(db: PgPool) {
    let app = TestApp::new(db.clone());
    let user = app.user("oliver", false).await;
    let eschbach = |title, index, cover| Seed {
        author: Some("Andreas Eschbach"),
        series: Some("Perry Rhodan"),
        series_index: Some(index),
        cover_path: cover,
        ..Seed::new("book", title)
    };
    let one = seed(&db, eschbach("Band 1", 1.0, Some("/covers/1.jpg"))).await;
    let two = seed(&db, eschbach("Band 2", 2.0, Some("/covers/2.jpg"))).await;
    let three = seed(&db, eschbach("Band 3", 3.0, Some("/covers/3.jpg"))).await;
    seed(&db, eschbach("Band 4 ohne Bild", 4.0, None)).await;
    seed(
        &db,
        Seed {
            author: Some("K. H. Scheer"),
            series: Some("Perry Rhodan"),
            series_index: Some(1.0),
            ..Seed::new("book", "Fremder Band")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            series: Some("Anonym"),
            ..Seed::new("book", "A")
        },
    )
    .await;
    seed(&db, Seed::new("book", "Solo")).await;
    seed(
        &db,
        Seed {
            missing: true,
            ..eschbach("Verschwunden", 5.0, None)
        },
    )
    .await;
    seed_progress(&db, user, one, 500.0, true).await;

    let rows = book_series(&db, user).await.unwrap();
    let keys: Vec<_> = rows
        .iter()
        .map(|r| (r.author.as_deref(), r.series.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            (Some("Andreas Eschbach"), "Perry Rhodan"),
            (Some("K. H. Scheer"), "Perry Rhodan"),
            (None, "Anonym")
        ]
    );
    assert_eq!(rows[0].count, 4);
    assert_eq!(rows[0].finished_count, 1);
    assert_eq!(rows[0].covers, Some(vec![one, two, three]));
    assert_eq!(rows[1].count, 1);
    assert_eq!(rows[1].covers, None);
}
