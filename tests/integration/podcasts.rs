//! Podcasts: store, download, relocate, retention and the JSON routes (legacy podcasts.test.ts,
//! podcasts-api.test.ts, podcast-preview-api.test.ts, podcast-retention.test.ts, the
//! podcastOverview cases of queries-aggregate.test.ts and directory.test.ts). Every outgoing
//! request goes to a local [`FakeWeb`], never to the internet.

use std::path::Path;

use axum::http::{Method, StatusCode};
use chrono::{TimeZone, Utc};
use serde_json::json;
use sqlx::PgPool;

use autoreverse_server::Config;
use autoreverse_server::library::queries::podcast_overview;
use autoreverse_server::podcasts::download::{DownloadError, download_episode};
use autoreverse_server::podcasts::http_client;
use autoreverse_server::podcasts::relocate::{RelocationResult, relocate_legacy_downloads};
use autoreverse_server::podcasts::retention::{
    RetentionResult, apply_retention, effective_keep, get_keep_default, refresh_all_and_retain,
    retain_for_podcast, set_keep_default, set_keep_for_podcast,
};
use autoreverse_server::podcasts::store::{FeedError, refresh, subscribe, unsubscribe};

use crate::common::{FakeWeb, Seed, TestApp, body_json, seed, seed_progress, seed_track};

const FEED: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Maschinenraum</title>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
</channel></rss>"#;

const NOT_A_FEED: &str = "<html>not a feed</html>";

/// The app with its data directory in `data` and the podcast directory on `web`.
fn app_with(db: PgPool, data: &Path, web: &FakeWeb) -> TestApp {
    let data = data.to_str().unwrap().to_owned();
    let mut config =
        Config::from_lookup(|key| (key == "AUTOREVERSE_DATA").then(|| data.clone())).unwrap();
    config.podcast_directory_url = web.url("/search");
    TestApp::with_config(db, config)
}

async fn episodes_of(db: &PgPool, podcast: i32) -> Vec<(i32, String, Option<String>)> {
    sqlx::query_as("SELECT id, title, chapters_url FROM items WHERE parent_id = $1 ORDER BY id")
        .bind(podcast)
        .fetch_all(db)
        .await
        .unwrap()
}

async fn episode(db: &PgPool, podcast: i32, title: &str, guid: &str, media: Option<&str>) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO items (kind, parent_id, title, sort_title, guid, feed_url)
         VALUES ('episode', $1, $2, lower($2), $3, $4) RETURNING id",
    )
    .bind(podcast)
    .bind(title)
    .bind(guid)
    .bind(media)
    .fetch_one(db)
    .await
    .unwrap()
}

async fn track_paths(db: &PgPool) -> Vec<(i32, String)> {
    sqlx::query_as("SELECT item_id, path FROM tracks ORDER BY item_id")
        .fetch_all(db)
        .await
        .unwrap()
}

// ---- store ----

#[sqlx::test]
async fn subscribe_creates_the_podcast_and_resubscribing_reuses_it(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let http = http_client();
    let first = subscribe(&db, &http, &web.url("/feed.xml"), None)
        .await
        .unwrap();
    let second = subscribe(&db, &http, &web.url("/feed.xml"), None)
        .await
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!((first.new_episodes, first.updated_episodes), (1, 0));
    assert_eq!((second.new_episodes, second.updated_episodes), (0, 1));
    assert_eq!(episodes_of(&db, first.id).await.len(), 1);
}

#[sqlx::test]
async fn subscribe_stores_podlove_chapters_and_keeps_the_json_chapter_url(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set(
        "/feed.xml",
        200,
        r#"<?xml version="1.0"?><rss version="2.0"
            xmlns:psc="http://podlove.org/simple-chapters"
            xmlns:podcast="https://podcastindex.org/namespace/1.0"
            xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>Maschinenraum</title>
            <item><title>Mit Kapiteln</title><guid>ep-1</guid><enclosure url="https://x/1.mp3"/>
                <itunes:duration>00:30:00</itunes:duration>
                <psc:chapters version="1.2">
                    <psc:chapter start="00:00:00" title="Begrüßung"/>
                    <psc:chapter start="00:10:00" title="Thema"/>
                </psc:chapters></item>
            <item><title>Nur URL</title><guid>ep-2</guid><enclosure url="https://x/2.mp3"/>
                <podcast:chapters url="https://x/2.json" type="application/json+chapters"/></item>
        </channel></rss>"#,
    );
    let podcast = subscribe(&db, &http_client(), &web.url("/feed.xml"), None)
        .await
        .unwrap();
    let episodes = episodes_of(&db, podcast.id).await;
    let with = episodes.iter().find(|e| e.1 == "Mit Kapiteln").unwrap();
    let rows: Vec<(i32, String, f64, f64)> = sqlx::query_as(
        "SELECT position, title, start, \"end\" FROM chapters WHERE item_id = $1 ORDER BY position",
    )
    .bind(with.0)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            (1, "Begrüßung".into(), 0.0, 600.0),
            // The last chapter runs to the length the feed declared.
            (2, "Thema".into(), 600.0, 1800.0)
        ]
    );
    // The JSON document is not fetched on a feed refresh, only its address is kept.
    let url_only = episodes.iter().find(|e| e.1 == "Nur URL").unwrap();
    assert_eq!(url_only.2.as_deref(), Some("https://x/2.json"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chapters WHERE item_id = $1")
        .bind(url_only.0)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

fn feed_with_art(web: &FakeWeb) -> String {
    FEED.replace(
        "<channel><title>Maschinenraum</title>",
        &format!(
            "<channel><title>Maschinenraum</title><itunes:image href=\"{}\"/>",
            web.url("/art.png")
        ),
    )
    .replace(
        "<rss version=\"2.0\">",
        "<rss xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\" version=\"2.0\">",
    )
}

#[sqlx::test]
async fn subscribe_stores_the_feed_artwork_in_the_covers_dir(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, feed_with_art(&web));
    web.set(
        "/art.png",
        200,
        vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00],
    );
    let covers = tempfile::tempdir().unwrap();
    let podcast = subscribe(
        &db,
        &http_client(),
        &web.url("/feed.xml"),
        Some(covers.path()),
    )
    .await
    .unwrap();
    let expected = covers.path().join(format!("{}.png", podcast.id));
    assert_eq!(podcast.cover_path.as_deref(), expected.to_str());
    assert!(expected.exists());
    let stored: Option<String> = sqlx::query_scalar("SELECT cover_path FROM items WHERE id = $1")
        .bind(podcast.id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored, podcast.cover_path);
}

#[sqlx::test]
async fn subscribe_succeeds_when_the_artwork_cannot_be_fetched(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, feed_with_art(&web));
    let covers = tempfile::tempdir().unwrap();
    let podcast = subscribe(
        &db,
        &http_client(),
        &web.url("/feed.xml"),
        Some(covers.path()),
    )
    .await
    .unwrap();
    assert!(podcast.id > 0);
    assert_eq!(podcast.cover_path, None);
}

#[sqlx::test]
async fn subscribe_and_refresh_refuse_a_body_that_is_no_feed(db: PgPool) {
    let web = FakeWeb::start().await;
    let http = http_client();
    web.set("/bad.xml", 200, NOT_A_FEED);
    assert!(matches!(
        subscribe(&db, &http, &web.url("/bad.xml"), None).await,
        Err(FeedError::Invalid(_))
    ));
    web.set("/feed.xml", 200, FEED);
    let podcast = subscribe(&db, &http, &web.url("/feed.xml"), None)
        .await
        .unwrap();
    web.set("/feed.xml", 200, NOT_A_FEED);
    assert!(matches!(
        refresh(&db, &http, podcast.id, None).await,
        Err(FeedError::Invalid(_))
    ));
}

#[sqlx::test]
async fn an_unreachable_feed_is_a_fetch_error(db: PgPool) {
    let web = FakeWeb::start().await;
    let http = http_client();
    let missing = subscribe(&db, &http, &web.url("/gone.xml"), None).await;
    assert!(matches!(missing, Err(FeedError::Fetch(ref m)) if m == "HTTP 404"));
    let refused = subscribe(&db, &http, "http://127.0.0.1:9/feed.xml", None).await;
    assert!(matches!(refused, Err(FeedError::Fetch(_))));
}

#[sqlx::test]
async fn refresh_dedupes_episodes_by_guid_and_updates_them(db: PgPool) {
    let web = FakeWeb::start().await;
    let http = http_client();
    web.set("/feed.xml", 200, FEED);
    let podcast = subscribe(&db, &http, &web.url("/feed.xml"), None)
        .await
        .unwrap();
    web.set(
        "/feed.xml",
        200,
        FEED.replace("Folge 118</title>", "Folge 118 (neu)</title>"),
    );
    let refreshed = refresh(&db, &http, podcast.id, None).await.unwrap();
    assert_eq!((refreshed.new_episodes, refreshed.updated_episodes), (0, 1));
    let episodes = episodes_of(&db, podcast.id).await;
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].1, "Folge 118 (neu)");
    assert!(matches!(
        refresh(&db, &http, 999_999, None).await,
        Err(FeedError::NotFound)
    ));
}

#[sqlx::test]
async fn unsubscribe_deletes_the_podcast_its_episodes_and_its_downloads(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "P")).await;
    let inside = episode(&db, podcast, "E1", "g1", None).await;
    let outside = episode(&db, podcast, "E2", "g2", None).await;
    let file = dir.path().join("P").join("E1.mp3");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "audio").unwrap();
    let foreign = elsewhere.path().join("x.mp3");
    std::fs::write(&foreign, "audio").unwrap();
    seed_track(&db, inside, 1, file.to_str().unwrap(), 10.0).await;
    seed_track(&db, outside, 1, foreign.to_str().unwrap(), 10.0).await;

    let report = unsubscribe(&db, podcast, dir.path()).await.unwrap();
    assert_eq!(
        (report.episodes, report.files_deleted, report.files_kept),
        (2, 1, 1)
    );
    assert!(!file.exists());
    assert!(!dir.path().join("P").exists(), "the empty folder goes too");
    assert!(foreign.exists());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert!(matches!(
        unsubscribe(&db, podcast, dir.path()).await,
        Err(FeedError::NotFound)
    ));
}

// ---- download ----

#[sqlx::test]
async fn download_refuses_an_episode_without_media_url(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "P")).await;
    let id = episode(&db, podcast, "E", "g1", None).await;
    let result = download_episode(&db, &http_client(), id, dir.path()).await;
    assert!(matches!(result, Err(DownloadError::NotDownloadable(_))));
}

#[sqlx::test]
async fn download_refuses_a_podcast_id(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, vec![0u8, 1, 2, 3]);
    let dir = tempfile::tempdir().unwrap();
    let podcast: i32 = sqlx::query_scalar(
        "INSERT INTO items (kind, title, sort_title, feed_url) VALUES ('podcast', 'P', 'p', $1) RETURNING id",
    )
    .bind(web.url("/feed.xml"))
    .fetch_one(&db)
    .await
    .unwrap();
    let result = download_episode(&db, &http_client(), podcast, dir.path()).await;
    assert!(matches!(result, Err(DownloadError::NotFound)));
    assert!(track_paths(&db).await.is_empty());
}

#[sqlx::test]
async fn download_writes_the_file_into_the_podcast_folder_and_adds_a_track(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/ep.mp3", 200, vec![0u8, 1, 2, 3]);
    let dir = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "P")).await;
    let id = episode(&db, podcast, "E", "g2", Some(&web.url("/ep.mp3"))).await;
    let result = download_episode(&db, &http_client(), id, dir.path())
        .await
        .unwrap();
    assert!(result.track_id > 0);
    let path = dir.path().join("P").join("E.mp3");
    assert_eq!(
        track_paths(&db).await,
        vec![(id, path.to_str().unwrap().to_owned())]
    );
    assert_eq!(std::fs::read(&path).unwrap(), vec![0u8, 1, 2, 3]);
    assert!(!dir.path().join("P").join("E.mp3.part").exists());

    // Again: the same file is replaced in place, not duplicated.
    download_episode(&db, &http_client(), id, dir.path())
        .await
        .unwrap();
    assert_eq!(track_paths(&db).await.len(), 1);
}

#[sqlx::test]
async fn download_keeps_two_episodes_with_the_same_title_apart(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/ep.mp3", 200, vec![0u8, 1, 2, 3]);
    let dir = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "Der / Podcast")).await;
    let first = episode(
        &db,
        podcast,
        "Folge: eins?",
        "g3",
        Some(&web.url("/ep.mp3")),
    )
    .await;
    let second = episode(
        &db,
        podcast,
        "Folge: eins?",
        "g4",
        Some(&web.url("/ep.mp3")),
    )
    .await;
    let http = http_client();
    download_episode(&db, &http, first, dir.path())
        .await
        .unwrap();
    download_episode(&db, &http, second, dir.path())
        .await
        .unwrap();
    let folder = dir.path().join("Der Podcast");
    assert_eq!(
        track_paths(&db).await,
        vec![
            (
                first,
                folder.join("Folge eins.mp3").to_str().unwrap().to_owned()
            ),
            (
                second,
                folder
                    .join(format!("Folge eins ({second}).mp3"))
                    .to_str()
                    .unwrap()
                    .to_owned()
            ),
        ]
    );
}

#[sqlx::test]
async fn download_takes_chapters_from_the_json_document(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/ep.mp3", 200, vec![0u8, 1, 2, 3]);
    web.set(
        "/ep.json",
        200,
        json!({ "chapters": [{ "startTime": 0, "title": "Intro" }, { "startTime": 60, "title": "Thema" }] })
            .to_string(),
    );
    let dir = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "P")).await;
    let id = episode(&db, podcast, "E", "g5", Some(&web.url("/ep.mp3"))).await;
    sqlx::query("UPDATE items SET chapters_url = $2 WHERE id = $1")
        .bind(id)
        .bind(web.url("/ep.json"))
        .execute(&db)
        .await
        .unwrap();
    download_episode(&db, &http_client(), id, dir.path())
        .await
        .unwrap();
    let titles: Vec<String> =
        sqlx::query_scalar("SELECT title FROM chapters WHERE item_id = $1 ORDER BY position")
            .bind(id)
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(titles, vec!["Intro", "Thema"]);
}

#[sqlx::test]
async fn a_failed_download_leaves_nothing_behind(db: PgPool) {
    let web = FakeWeb::start().await;
    let dir = tempfile::tempdir().unwrap();
    let podcast = seed(&db, Seed::new("podcast", "P")).await;
    let id = episode(&db, podcast, "E", "g6", Some(&web.url("/missing.mp3"))).await;
    let result = download_episode(&db, &http_client(), id, dir.path()).await;
    assert!(matches!(result, Err(DownloadError::Fetch(ref m)) if m == "HTTP 404"));
    assert!(track_paths(&db).await.is_empty());
    assert!(!dir.path().join("P").join("E.mp3").exists());
}

// ---- relocate ----

/// An episode with its old-style flat download on disk and in the tracks table.
async fn legacy_download(
    db: &PgPool,
    dir: &Path,
    podcast_title: &str,
    title: &str,
) -> (i32, String) {
    let podcast = seed(db, Seed::new("podcast", podcast_title)).await;
    let id = episode(
        db,
        podcast,
        title,
        &format!("guid-{title}"),
        Some("https://x/ep.mp3"),
    )
    .await;
    let path = dir.join(format!("{id}.mp3"));
    std::fs::write(&path, "audio").unwrap();
    seed_track(db, id, 1, path.to_str().unwrap(), 10.0).await;
    (id, path.to_str().unwrap().to_owned())
}

#[sqlx::test]
async fn relocate_moves_an_old_flat_download_into_its_podcast_folder(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let (id, old) = legacy_download(&db, dir.path(), "Maschinenraum", "Folge 118").await;
    let result = relocate_legacy_downloads(&db, dir.path()).await.unwrap();
    assert_eq!(
        result,
        RelocationResult {
            moved: 1,
            skipped: 0,
            failed: 0
        }
    );
    let moved = dir.path().join("Maschinenraum").join("Folge 118.mp3");
    assert!(moved.exists());
    assert!(!Path::new(&old).exists());
    assert_eq!(
        track_paths(&db).await,
        vec![(id, moved.to_str().unwrap().to_owned())]
    );
}

#[sqlx::test]
async fn relocate_has_nothing_left_to_do_on_a_second_run(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    legacy_download(&db, dir.path(), "P", "E").await;
    let once = relocate_legacy_downloads(&db, dir.path()).await.unwrap();
    assert_eq!(
        once,
        RelocationResult {
            moved: 1,
            skipped: 0,
            failed: 0
        }
    );
    let twice = relocate_legacy_downloads(&db, dir.path()).await.unwrap();
    assert_eq!(
        twice,
        RelocationResult {
            moved: 0,
            skipped: 1,
            failed: 0
        }
    );
    assert!(dir.path().join("P").join("E.mp3").exists());
}

#[sqlx::test]
async fn relocate_leaves_a_file_outside_the_podcasts_root_alone(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (id, path) = legacy_download(&db, elsewhere.path(), "P", "E").await;
    let result = relocate_legacy_downloads(&db, dir.path()).await.unwrap();
    assert_eq!(
        result,
        RelocationResult {
            moved: 0,
            skipped: 1,
            failed: 0
        }
    );
    assert!(Path::new(&path).exists());
    assert_eq!(track_paths(&db).await, vec![(id, path)]);
}

// ---- retention ----

async fn show(db: &PgPool, title: &str, feed_url: &str) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO items (kind, title, sort_title, feed_url) VALUES ('podcast', $1, lower($1), $2)
         RETURNING id",
    )
    .bind(title)
    .bind(feed_url)
    .fetch_one(db)
    .await
    .unwrap()
}

async fn dated_episode(db: &PgPool, podcast: i32, day: u32, file: Option<&Path>) -> i32 {
    let id: i32 = sqlx::query_scalar(
        "INSERT INTO items (kind, parent_id, title, sort_title, published_at)
         VALUES ('episode', $1, $2, lower($2), $3) RETURNING id",
    )
    .bind(podcast)
    .bind(format!("Folge {day}"))
    .bind(Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap())
    .fetch_one(db)
    .await
    .unwrap();
    if let Some(file) = file {
        seed_track(db, id, 1, file.to_str().unwrap(), 60.0).await;
    }
    id
}

fn temp_file(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, "audio").unwrap();
    path
}

#[sqlx::test]
async fn retention_deletes_downloads_past_the_newest_n(db: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let podcast = show(&db, "Show", "https://x/feed").await;
    let old = temp_file(&files, "old.mp3");
    let new = temp_file(&files, "new.mp3");
    dated_episode(&db, podcast, 1, Some(&old)).await;
    let newer = dated_episode(&db, podcast, 9, Some(&new)).await;
    let result = apply_retention(&db, &http_client(), podcast, 1, Path::new("/tmp"))
        .await
        .unwrap();
    assert_eq!(result.freed, 1);
    let left: Vec<i32> = track_paths(&db).await.into_iter().map(|t| t.0).collect();
    assert_eq!(left, vec![newer]);
    assert!(!old.exists());
    assert!(new.exists());
}

#[sqlx::test]
async fn retention_keeps_everything_when_the_number_is_high_enough(db: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let podcast = show(&db, "Show", "https://x/feed").await;
    dated_episode(&db, podcast, 1, Some(&temp_file(&files, "a.mp3"))).await;
    dated_episode(&db, podcast, 2, Some(&temp_file(&files, "b.mp3"))).await;
    let result = apply_retention(&db, &http_client(), podcast, 10, Path::new("/tmp"))
        .await
        .unwrap();
    assert_eq!(result.freed, 0);
    assert_eq!(track_paths(&db).await.len(), 2);
}

#[sqlx::test]
async fn retention_zero_keeps_nothing_downloaded(db: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let podcast = show(&db, "Show", "https://x/feed").await;
    dated_episode(&db, podcast, 1, Some(&temp_file(&files, "a.mp3"))).await;
    let result = apply_retention(&db, &http_client(), podcast, 0, Path::new("/tmp"))
        .await
        .unwrap();
    assert_eq!(result.freed, 1);
    assert!(track_paths(&db).await.is_empty());
}

#[sqlx::test]
async fn retention_counts_a_failed_download_instead_of_aborting(db: PgPool) {
    let podcast = show(&db, "Show", "https://x/feed").await;
    dated_episode(&db, podcast, 1, None).await;
    dated_episode(&db, podcast, 2, None).await;
    let result = apply_retention(&db, &http_client(), podcast, 2, Path::new("/tmp"))
        .await
        .unwrap();
    assert_eq!((result.failed, result.downloaded), (2, 0));
}

#[sqlx::test]
async fn retention_downloads_the_newest_missing_episodes(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/2.mp3", 200, vec![1u8, 2, 3]);
    let dir = tempfile::tempdir().unwrap();
    let podcast = show(&db, "Show", "https://x/feed").await;
    dated_episode(&db, podcast, 1, None).await;
    let newest = dated_episode(&db, podcast, 2, None).await;
    sqlx::query("UPDATE items SET feed_url = $2 WHERE id = $1")
        .bind(newest)
        .bind(web.url("/2.mp3"))
        .execute(&db)
        .await
        .unwrap();
    let result = apply_retention(&db, &http_client(), podcast, 1, dir.path())
        .await
        .unwrap();
    assert_eq!(
        result,
        RetentionResult {
            downloaded: 1,
            freed: 0,
            failed: 0
        }
    );
    assert_eq!(web.hits("/2.mp3"), 1);
}

#[sqlx::test]
async fn the_default_applies_unless_a_podcast_overrides_it(db: PgPool) {
    let podcast = show(&db, "Show", "https://x/feed").await;
    assert_eq!(get_keep_default(&db).await.unwrap(), 0);
    set_keep_default(&db, 5.0).await.unwrap();
    assert_eq!(get_keep_default(&db).await.unwrap(), 5);
    assert_eq!(effective_keep(&db, None).await.unwrap(), 5);
    assert_eq!(effective_keep(&db, Some(2)).await.unwrap(), 2);

    set_keep_for_podcast(&db, podcast, Some(3.0)).await.unwrap();
    let keep: Option<i32> = sqlx::query_scalar("SELECT keep_episodes FROM items WHERE id = $1")
        .bind(podcast)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(keep, Some(3));
    set_keep_for_podcast(&db, podcast, None).await.unwrap();
    let keep: Option<i32> = sqlx::query_scalar("SELECT keep_episodes FROM items WHERE id = $1")
        .bind(podcast)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(keep, None);
}

#[sqlx::test]
async fn an_out_of_range_default_is_clamped(db: PgPool) {
    set_keep_default(&db, 999.0).await.unwrap();
    assert_eq!(get_keep_default(&db).await.unwrap(), 50);
    set_keep_default(&db, -4.0).await.unwrap();
    assert_eq!(get_keep_default(&db).await.unwrap(), 0);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM library_config")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[sqlx::test]
async fn retain_for_podcast_uses_the_override_and_ignores_other_kinds(db: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let podcast = show(&db, "Show", "https://x/feed").await;
    set_keep_default(&db, 10.0).await.unwrap();
    set_keep_for_podcast(&db, podcast, Some(1.0)).await.unwrap();
    dated_episode(&db, podcast, 1, Some(&temp_file(&files, "a.mp3"))).await;
    dated_episode(&db, podcast, 2, Some(&temp_file(&files, "b.mp3"))).await;
    let http = http_client();
    let result = retain_for_podcast(&db, &http, podcast, Path::new("/tmp"))
        .await
        .unwrap();
    assert_eq!(result.freed, 1);
    let album = seed(&db, Seed::new("album", "A")).await;
    assert_eq!(
        retain_for_podcast(&db, &http, album, Path::new("/tmp"))
            .await
            .unwrap(),
        RetentionResult::default()
    );
}

#[sqlx::test]
async fn the_scheduled_run_counts_new_episodes_and_names_the_failed_feed(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set(
        "/feed",
        200,
        r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Maschinenraum</title>
        <item><title>Neue Folge</title><guid>ep-1</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
        <enclosure url="https://example.com/1.mp3" type="audio/mpeg" length="123"/></item></channel></rss>"#,
    );
    let reachable = show(&db, "Show", &web.url("/feed")).await;
    let unreachable = show(&db, "Weg", "http://127.0.0.1:9/feed").await;
    let dir = tempfile::tempdir().unwrap();
    let run = refresh_all_and_retain(&db, &http_client(), dir.path(), dir.path())
        .await
        .unwrap();
    assert_eq!(run.podcasts, 2);
    assert_eq!(run.new_episodes, 1);
    assert_eq!(run.failed, 1);
    assert_eq!(run.errors.len(), 1);
    assert!(
        run.errors[0].contains(&unreachable.to_string()),
        "{:?}",
        run.errors
    );
    assert!(
        run.errors[0].contains("nicht erreichbar"),
        "{:?}",
        run.errors
    );
    assert_eq!(episodes_of(&db, reachable).await.len(), 1);
}

// ---- podcastOverview ----

#[sqlx::test]
async fn overview_counts_as_unheard_only_episodes_since_the_subscription(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let podcast = seed(
        app.db(),
        Seed {
            added_at: Some(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()),
            ..Seed::new("podcast", "Maschinenraum")
        },
    )
    .await;
    for (title, guid, published) in [
        (
            "Alt",
            "old",
            Some(Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap()),
        ),
        (
            "Neu",
            "new",
            Some(Utc.with_ymd_and_hms(2026, 3, 5, 0, 0, 0).unwrap()),
        ),
        ("Undatiert", "undated", None),
    ] {
        let id = episode(app.db(), podcast, title, guid, None).await;
        sqlx::query("UPDATE items SET published_at = $2 WHERE id = $1")
            .bind(id)
            .bind(published)
            .execute(app.db())
            .await
            .unwrap();
    }
    let rows = podcast_overview(app.db(), user).await.unwrap();
    assert_eq!((rows[0].episode_count, rows[0].unheard_count), (3, 1));

    let new: i32 = sqlx::query_scalar("SELECT id FROM items WHERE guid = 'new'")
        .fetch_one(app.db())
        .await
        .unwrap();
    seed_progress(app.db(), user, new, 10.0, true).await;
    let rows = podcast_overview(app.db(), user).await.unwrap();
    assert_eq!(rows[0].unheard_count, 0);
}

// ---- routes ----

async fn admin(app: &TestApp) -> String {
    let id = app.user("oliver", true).await;
    app.token(id).await
}

#[sqlx::test]
async fn post_podcasts_subscribes_and_answers_with_the_podcast(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let response = app
        .post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/feed.xml") }),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["title"], "Maschinenraum");
    assert_eq!(body["feed_url"], web.url("/feed.xml"));
    assert_eq!(body["new_episodes"], 1);
    assert_eq!(body["updated_episodes"], 0);
    assert!(body["id"].as_i64().unwrap() > 0);
}

#[sqlx::test]
async fn post_podcasts_answers_502_and_422_for_broken_feeds(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/bad.xml", 200, NOT_A_FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let unreachable = app
        .post_json(
            "/podcasts",
            json!({ "feed_url": "http://127.0.0.1:9/feed.xml" }),
            Some(&token),
        )
        .await;
    assert_eq!(unreachable.status(), StatusCode::BAD_GATEWAY);
    let missing = app
        .post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/gone.xml") }),
            Some(&token),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(body_json(missing).await["detail"], "HTTP 404");
    let invalid = app
        .post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/bad.xml") }),
            Some(&token),
        )
        .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    for body in [
        json!({}),
        json!({ "feed_url": "" }),
        json!({ "feed_url": 3 }),
        json!(null),
    ] {
        let response = app.post_json("/podcasts", body.clone(), Some(&token)).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(
            body_json(response).await["detail"],
            "feed_url muss eine nicht-leere Zeichenkette sein"
        );
    }
}

#[sqlx::test]
async fn a_non_admin_may_subscribe(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    app.user("admin", true).await;
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    let response = app
        .post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/feed.xml") }),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_routes_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for (method, uri) in [
        (Method::POST, "/podcasts"),
        (Method::DELETE, "/podcasts/1"),
        (Method::POST, "/podcasts/1/refresh"),
        (Method::POST, "/podcasts/preview"),
        (Method::GET, "/podcasts/search?q=x"),
        (Method::POST, "/episodes/1/download"),
    ] {
        let response = app
            .call(
                method,
                uri,
                Some(json!({ "feed_url": "https://x/feed.xml" })),
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[sqlx::test]
async fn delete_podcast_answers_200_with_the_report(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let created = body_json(
        app.post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/feed.xml") }),
            Some(&token),
        )
        .await,
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let response = app
        .call(
            Method::DELETE,
            &format!("/podcasts/{id}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "episodes": 1, "files_deleted": 0, "files_kept": 0 })
    );
    for uri in ["/podcasts/999999", "/podcasts/abc"] {
        let response = app.call(Method::DELETE, uri, None, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["detail"], "Unbekannter Podcast");
    }
}

#[sqlx::test]
async fn refresh_answers_with_counts_and_422_for_a_broken_feed(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/feed.xml", 200, FEED);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let created = body_json(
        app.post_json(
            "/podcasts",
            json!({ "feed_url": web.url("/feed.xml") }),
            Some(&token),
        )
        .await,
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let response = app
        .call(
            Method::POST,
            &format!("/podcasts/{id}/refresh"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["new_episodes"], 0);
    assert_eq!(body["updated_episodes"], 1);
    assert_eq!(body["downloaded"], 0);
    assert_eq!(body["freed"], 0);

    web.set("/feed.xml", 200, NOT_A_FEED);
    let response = app
        .call(
            Method::POST,
            &format!("/podcasts/{id}/refresh"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = app
        .call(Method::POST, "/podcasts/999999/refresh", None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await["detail"], "Unbekannter Podcast");
}

#[sqlx::test]
async fn download_route_answers_like_legacy(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/ep.mp3", 200, vec![0u8, 1, 2, 3]);
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let response = app
        .call(
            Method::POST,
            "/episodes/999999/download",
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await["detail"], "Unbekannte Folge");

    let podcast = seed(app.db(), Seed::new("podcast", "P")).await;
    let bare = episode(app.db(), podcast, "E", "g1", None).await;
    let response = app
        .call(
            Method::POST,
            &format!("/episodes/{bare}/download"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let gone = episode(app.db(), podcast, "G", "g2", Some(&web.url("/gone.mp3"))).await;
    let response = app
        .call(
            Method::POST,
            &format!("/episodes/{gone}/download"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let fine = episode(app.db(), podcast, "F", "g3", Some(&web.url("/ep.mp3"))).await;
    let response = app
        .call(
            Method::POST,
            &format!("/episodes/{fine}/download"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["track_id"].as_i64().unwrap() > 0);
    assert_eq!(body["duration"], 0);
    assert!(
        data.path()
            .join("podcasts")
            .join("P")
            .join("F.mp3")
            .exists()
    );
}

#[sqlx::test]
async fn a_downloaded_episode_plays_and_streams(db: PgPool) {
    let web = FakeWeb::start().await;
    let audio = tempfile::tempdir().unwrap();
    let mp3 = audio.path().join("one.mp3");
    crate::common::make_audio(&mp3, &[]);
    web.set("/ep.mp3", 200, std::fs::read(&mp3).unwrap());
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let token = admin(&app).await;
    let podcast = seed(app.db(), Seed::new("podcast", "P")).await;
    let id = episode(app.db(), podcast, "E", "g1", Some(&web.url("/ep.mp3"))).await;

    let before = app
        .call(Method::POST, &format!("/play/{id}"), None, Some(&token))
        .await;
    assert_eq!(before.status(), StatusCode::CONFLICT);
    let downloaded = body_json(
        app.call(
            Method::POST,
            &format!("/episodes/{id}/download"),
            None,
            Some(&token),
        )
        .await,
    )
    .await;
    assert!(downloaded["duration"].as_f64().unwrap() > 0.5);

    let play = app
        .call(Method::POST, &format!("/play/{id}"), None, Some(&token))
        .await;
    assert_eq!(play.status(), StatusCode::OK);
    let body = body_json(play).await;
    assert_eq!(body["kind"], "episode");
    let track = body["tracks"][0]["id"].as_i64().unwrap();
    let stream = app
        .get(&format!("/tracks/{track}/stream"), Some(&token))
        .await;
    assert_eq!(stream.status(), StatusCode::OK);
}

#[sqlx::test]
async fn preview_returns_the_feed_without_subscribing(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set(
        "/feed.xml",
        200,
        r#"<?xml version="1.0"?><rss version="2.0"><channel>
<title>Maschinenraum</title>
<description>Ein Podcast über Technik.</description>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
<item><title>Folge 117</title><guid>ep-117</guid><pubDate>Thu, 25 Dec 2025 10:00:00 GMT</pubDate><enclosure url="https://x/117.mp3"/></item>
</channel></rss>"#,
    );
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    let response = app
        .post_json(
            "/podcasts/preview",
            json!({ "feed_url": web.url("/feed.xml") }),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["title"], "Maschinenraum");
    assert_eq!(body["description"], "Ein Podcast über Technik.");
    assert_eq!(body["image_url"], serde_json::Value::Null);
    let titles: Vec<&str> = body["episodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Folge 118", "Folge 117"]);
    assert_eq!(body["episodes"][0]["published_at"], "2026-01-01T10:00:00Z");
    assert_eq!(body["episodes"][0]["duration"], serde_json::Value::Null);

    let items: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(items, 0, "a preview is not a subscription");

    let missing = app
        .post_json("/podcasts/preview", json!({}), Some(&token))
        .await;
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let unreachable = app
        .post_json(
            "/podcasts/preview",
            json!({ "feed_url": "http://127.0.0.1:9/feed.xml" }),
            Some(&token),
        )
        .await;
    assert_eq!(unreachable.status(), StatusCode::BAD_GATEWAY);
}

#[sqlx::test]
async fn search_asks_the_directory_and_maps_the_results(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set(
        "/search",
        200,
        json!({ "results": [
            { "collectionName": "Maschinenraum", "artistName": "M. Raum",
              "feedUrl": "https://x/feed.xml", "artworkUrl100": "https://x/art.png",
              "trackCount": 118, "genres": ["Technology"] },
            { "collectionName": "No Feed" }
        ]})
        .to_string(),
    );
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    let response = app
        .get("/podcasts/search?q=maschinenraum", Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "results": [{
            "name": "Maschinenraum", "author": "M. Raum", "feed_url": "https://x/feed.xml",
            "artwork_url": "https://x/art.png", "episode_count": 118, "genre": "Technology"
        }]})
    );
    let asked = web.requests.lock().unwrap().last().cloned().unwrap();
    for part in [
        "media=podcast",
        "entity=podcast",
        "term=maschinenraum",
        "limit=25",
        "country=DE",
    ] {
        assert!(asked.contains(part), "{asked}");
    }

    app.get("/podcasts/search?q=x&limit=5&country=AT", Some(&token))
        .await;
    let asked = web.requests.lock().unwrap().last().cloned().unwrap();
    assert!(
        asked.contains("limit=5") && asked.contains("country=AT"),
        "{asked}"
    );
}

#[sqlx::test]
async fn search_validates_its_parameters_and_reports_a_broken_directory(db: PgPool) {
    let web = FakeWeb::start().await;
    web.set("/search", 200, "kein json");
    let data = tempfile::tempdir().unwrap();
    let app = app_with(db, data.path(), &web);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    let long = "x".repeat(201);
    for (uri, detail) in [
        ("/podcasts/search".to_owned(), "q muss 1–200 Zeichen haben"),
        (
            "/podcasts/search?q=".to_owned(),
            "q muss 1–200 Zeichen haben",
        ),
        (
            format!("/podcasts/search?q={long}"),
            "q muss 1–200 Zeichen haben",
        ),
        (
            "/podcasts/search?q=x&limit=51".to_owned(),
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/podcasts/search?q=x&country=DEU".to_owned(),
            "country muss genau 2 Zeichen haben",
        ),
    ] {
        let response = app.get(&uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert_eq!(body_json(response).await["detail"], detail, "{uri}");
    }
    let response = app.get("/podcasts/search?q=x", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        body_json(response).await["detail"],
        "Unerwartete Antwort des Podcast-Verzeichnisses"
    );
}
