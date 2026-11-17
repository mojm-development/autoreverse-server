//! `GET /tracks/{id}/stream?quality=…&format=…`: a smaller copy for a slow connection
//! (`streaming::transcode`). Needs ffmpeg — it builds the fixtures and it is what encodes.

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use sqlx::PgPool;

use autoreverse_server::config::Config;

use crate::common::{Seed, TestApp, body_json, header_str, seed, seed_track};

/// Four seconds of noise as FLAC: incompressible, so far above every target bitrate.
fn flac(path: &Path) {
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("anoisesrc=sample_rate=44100:duration=4:amplitude=0.5")
        .args(["-ac", "2"])
        .arg(path)
        .status()
        .expect("ffmpeg must be installed");
    assert!(status.success());
}

struct Fixture {
    app: TestApp,
    token: String,
    track: i32,
    source: std::path::PathBuf,
    data: tempfile::TempDir,
    _music: tempfile::TempDir,
}

async fn fixture(db: PgPool) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let source = music.path().join("song.flac");
    flac(&source);
    let dir = data.path().to_str().unwrap().to_owned();
    let config =
        Config::from_lookup(|key| (key == "AUTOREVERSE_DATA").then(|| dir.clone())).unwrap();
    let app = TestApp::with_config(db, config);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    let album = seed(app.db(), Seed::new("album", "A")).await;
    let track = seed_track(app.db(), album, 1, source.to_str().unwrap(), 4.0).await;
    Fixture {
        app,
        token,
        track,
        source,
        data,
        _music: music,
    }
}

impl Fixture {
    async fn stream(&self, query: &str, range: Option<&str>) -> Response<Body> {
        let mut request = Request::get(format!("/tracks/{}/stream{query}", self.track))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        self.app.send(request.body(Body::empty()).unwrap()).await
    }

    fn copies(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.data.path().join("transcodes")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

async fn bytes(response: Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

fn length(response: &Response<Body>) -> u64 {
    header_str(response, header::CONTENT_LENGTH)
        .parse()
        .unwrap()
}

#[sqlx::test]
async fn without_a_quality_the_file_goes_out_as_it_is(db: PgPool) {
    let f = fixture(db).await;
    let original = std::fs::metadata(&f.source).unwrap().len();
    for query in ["", "?quality=original", "?format=aac"] {
        let response = f.stream(query, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{query}");
        assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/flac");
        assert_eq!(length(&response), original);
    }
    assert!(f.copies().is_empty());
}

#[sqlx::test]
async fn a_quality_answers_with_a_smaller_whole_file_in_opus(db: PgPool) {
    let f = fixture(db).await;
    let original = std::fs::metadata(&f.source).unwrap().len();
    let response = f.stream("?quality=low", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/ogg");
    assert_eq!(header_str(&response, header::ACCEPT_RANGES), "bytes");
    let size = length(&response);
    assert!(size < original / 4, "{size} of {original}");
    let body = bytes(response).await;
    assert_eq!(body.len() as u64, size);
    assert_eq!(&body[..4], b"OggS");
}

#[sqlx::test]
async fn aac_comes_as_an_mp4_with_its_index_in_front(db: PgPool) {
    let f = fixture(db).await;
    let response = f.stream("?quality=medium&format=aac", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/mp4");
    let body = bytes(response).await;
    assert_eq!(&body[4..8], b"ftyp");
    let moov = body.windows(4).position(|w| w == b"moov").unwrap();
    let mdat = body.windows(4).position(|w| w == b"mdat").unwrap();
    assert!(moov < mdat, "moov at {moov}, mdat at {mdat}");
}

#[sqlx::test]
async fn the_copy_is_made_once_and_then_seeked_in_like_any_file(db: PgPool) {
    let f = fixture(db).await;
    let first = f.stream("?quality=low", None).await;
    let size = length(&first);
    let whole = bytes(first).await;
    let made = f.copies();
    assert_eq!(made.len(), 1, "{made:?}");

    let slice = f.stream("?quality=low", Some("bytes=100-299")).await;
    assert_eq!(slice.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        header_str(&slice, header::CONTENT_RANGE),
        format!("bytes 100-299/{size}")
    );
    assert_eq!(bytes(slice).await, whole[100..300]);
    // Still the one file: the second request did not encode again.
    assert_eq!(f.copies(), made);
    let after = std::fs::read(f.data.path().join("transcodes").join(&made[0])).unwrap();
    assert_eq!(after, whole);
}

#[sqlx::test]
async fn requests_at_the_same_time_share_one_encoding(db: PgPool) {
    let f = fixture(db).await;
    let (a, b, c) = tokio::join!(
        f.stream("?quality=high", None),
        f.stream("?quality=high", Some("bytes=0-99")),
        f.stream("?quality=high", None),
    );
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(b.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(length(&a), length(&c));
    assert_eq!(f.copies().len(), 1, "{:?}", f.copies());
}

#[sqlx::test]
async fn each_quality_and_format_has_a_copy_of_its_own(db: PgPool) {
    let f = fixture(db).await;
    let low = length(&f.stream("?quality=low", None).await);
    let high = length(&f.stream("?quality=high", None).await);
    f.stream("?quality=low&format=aac", None).await;
    assert!(low < high, "{low} / {high}");
    assert_eq!(f.copies().len(), 3, "{:?}", f.copies());
}

#[sqlx::test]
async fn a_source_that_is_small_already_is_not_encoded_again(db: PgPool) {
    let f = fixture(db).await;
    // The same file, said to run for two hours: a few kbit/s, below every target.
    sqlx::query("UPDATE tracks SET duration = 7200 WHERE id = $1")
        .bind(f.track)
        .execute(f.app.db())
        .await
        .unwrap();
    let response = f.stream("?quality=low", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/flac");
    assert!(f.copies().is_empty());
}

#[sqlx::test]
async fn a_file_that_cannot_be_encoded_goes_out_as_it_is(db: PgPool) {
    let f = fixture(db).await;
    // Large enough to count as worth it, and not audio at all.
    std::fs::write(&f.source, vec![7u8; 400_000]).unwrap();
    let response = f.stream("?quality=low", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/flac");
    assert_eq!(length(&response), 400_000);
    assert!(f.copies().is_empty(), "{:?}", f.copies());
}

#[sqlx::test]
async fn a_changed_file_gets_a_new_copy(db: PgPool) {
    let f = fixture(db).await;
    f.stream("?quality=low", None).await;
    let before = f.copies();
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("anoisesrc=sample_rate=44100:duration=6:amplitude=0.5")
        .args(["-ac", "2"])
        .arg(&f.source)
        .status()
        .unwrap();
    assert!(status.success());
    f.stream("?quality=low", None).await;
    let after = f.copies();
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(before.iter().all(|name| after.contains(name)));
}

#[sqlx::test]
async fn unknown_values_are_refused_in_german(db: PgPool) {
    let f = fixture(db).await;
    let quality = f.stream("?quality=best", None).await;
    assert_eq!(quality.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(quality).await["detail"], "Unbekannte Qualität");
    let format = f.stream("?quality=low&format=mp3", None).await;
    assert_eq!(format.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(format).await["detail"], "Unbekanntes Format");
}

#[sqlx::test]
async fn a_quality_still_needs_a_login_and_an_existing_file(db: PgPool) {
    let f = fixture(db).await;
    let anonymous = f
        .app
        .get(&format!("/tracks/{}/stream?quality=low", f.track), None)
        .await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    std::fs::remove_file(&f.source).unwrap();
    let gone = f.stream("?quality=low", None).await;
    assert_eq!(gone.status(), StatusCode::GONE);
}
