//! Loudness normalisation: the values read from tags and measured from real files, the
//! background pass (resuming, broken files, the switch) and the three fields in every track
//! shape of the API. Needs ffmpeg — for the tagged fixtures, and because it decodes.

use std::path::Path;

use autoreverse_server::library::paths::set_library_paths;
use autoreverse_server::scanner::loudness::{self, LoudnessProgress};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TempLibrary, TestApp, body_json, seed, seed_track};

/// A 16-bit stereo WAV with a 1 kHz sine at `dbfs`, written by hand: its loudness is known
/// exactly (EBU Tech 3341: a stereo sine at -23 dBFS reads -23.0 LUFS).
fn write_sine(path: &Path, seconds: f64, dbfs: f64) {
    let rate = 44_100u32;
    let frames = (seconds * f64::from(rate)) as u32;
    let amplitude = 10f64.powf(dbfs / 20.0) * 32767.0;
    let mut out = Vec::with_capacity(44 + frames as usize * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames * 4).to_le_bytes());
    for n in 0..frames {
        let t = f64::from(n) / f64::from(rate);
        let sample = (amplitude * (std::f64::consts::TAU * 1000.0 * t).sin()).round() as i16;
        out.extend_from_slice(&sample.to_le_bytes());
        out.extend_from_slice(&sample.to_le_bytes());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, out).unwrap();
}

/// Four seconds of a sine in whatever format the extension says, with tags, by ffmpeg.
fn encode_sine(path: &Path, tags: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut command = std::process::Command::new("ffmpeg");
    command.args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"]);
    command.arg("sine=frequency=1000:sample_rate=48000:duration=4");
    // The same signal on both channels at its full level (`-ac 2` would take 3 dB off): ffmpeg's
    // sine is at 1/8 of full scale, so this is a stereo sine at -18.06 dBFS.
    command.args(["-af", "pan=stereo|c0=c0|c1=c0"]);
    for (key, value) in tags {
        command.arg("-metadata").arg(format!("{key}={value}"));
    }
    let status = command
        .arg(path)
        .status()
        .expect("ffmpeg must be installed");
    assert!(status.success(), "ffmpeg failed for {}", path.display());
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Row {
    gain_db: Option<f64>,
    album_gain_db: Option<f64>,
    peak: Option<f64>,
    loudness_state: Option<String>,
}

async fn row(app: &TestApp, track: i32) -> Row {
    sqlx::query_as("SELECT gain_db, album_gain_db, peak, loudness_state FROM tracks WHERE id = $1")
        .bind(track)
        .fetch_one(app.db())
        .await
        .unwrap()
}

fn state(row: &Row) -> Option<&str> {
    row.loudness_state.as_deref()
}

fn near(value: Option<f64>, expected: f64, tolerance: f64) -> bool {
    value.is_some_and(|v| (v - expected).abs() <= tolerance)
}

/// An item with one track per path, in order.
async fn item_of(app: &TestApp, kind: &str, title: &str, paths: &[&Path]) -> (i32, Vec<i32>) {
    let item = seed(app.db(), Seed::new(kind, title)).await;
    let mut tracks = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let path = path.to_str().unwrap();
        tracks.push(seed_track(app.db(), item, index as i32 + 1, path, 5.0).await);
    }
    (item, tracks)
}

async fn read_tags(app: &TestApp) -> loudness::TagReport {
    loudness::read_pending_tags(app.db(), || false)
        .await
        .unwrap()
}

// --- tags --------------------------------------------------------------------------------------

#[sqlx::test]
async fn tags_of_real_files_are_read_without_measuring(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let gains = [
        ("REPLAYGAIN_TRACK_GAIN", "-6.48 dB"),
        ("REPLAYGAIN_ALBUM_GAIN", "-5,10 dB"),
        ("REPLAYGAIN_TRACK_PEAK", "0.977000"),
        ("REPLAYGAIN_ALBUM_PEAK", "1.000000"),
    ];
    // ID3 TXXX, Vorbis comments in FLAC and in Ogg.
    let mp3 = dir.path().join("a.mp3");
    let flac = dir.path().join("b.flac");
    let ogg = dir.path().join("c.ogg");
    for path in [&mp3, &flac, &ogg] {
        encode_sine(path, &gains);
    }
    // Lower case, as some taggers write the keys.
    let lower = dir.path().join("d.flac");
    encode_sine(
        &lower,
        &[
            ("replaygain_track_gain", "+1.50 dB"),
            ("replaygain_album_gain", "0.25"),
        ],
    );
    // Opus: Q7.8 towards -23 LUFS.
    let opus = dir.path().join("e.opus");
    encode_sine(
        &opus,
        &[("R128_TRACK_GAIN", "-1792"), ("R128_ALBUM_GAIN", "-1280")],
    );
    let plain = dir.path().join("f.flac");
    encode_sine(&plain, &[("title", "Ohne")]);
    let (_, tracks) = item_of(
        &app,
        "album",
        "Getaggt",
        &[&mp3, &flac, &ogg, &lower, &opus, &plain],
    )
    .await;

    let report = read_tags(&app).await;
    assert_eq!((report.read, report.pending), (6, 1));

    for track in &tracks[..3] {
        assert_eq!(
            row(&app, *track).await,
            Row {
                gain_db: Some(-6.48),
                album_gain_db: Some(-5.1),
                peak: Some(0.977),
                loudness_state: Some("done".into()),
            }
        );
    }
    let lower = row(&app, tracks[3]).await;
    assert_eq!(
        (lower.gain_db, lower.album_gain_db),
        (Some(1.5), Some(0.25))
    );
    assert_eq!(lower.peak, None);
    let opus = row(&app, tracks[4]).await;
    assert_eq!((opus.gain_db, opus.album_gain_db), (Some(-2.0), Some(0.0)));
    assert_eq!(state(&opus), Some("done"));
    let plain = row(&app, tracks[5]).await;
    assert_eq!((plain.gain_db, state(&plain)), (None, Some("pending")));

    // Once per version of a file: nothing is read twice.
    assert_eq!(read_tags(&app).await.read, 0);
}

#[sqlx::test]
async fn mp4_and_ape_tags_are_read(db: PgPool) {
    use lofty::ape::{ApeItem, ApeTag};
    use lofty::config::WriteOptions;
    use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
    use lofty::prelude::*;
    use lofty::tag::ItemValue;

    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    // The free-form atoms foobar2000 and MusicBrainz Picard write.
    let m4a = dir.path().join("a.m4a");
    encode_sine(&m4a, &[]);
    let mut ilst = Ilst::default();
    for (name, value) in [
        ("REPLAYGAIN_TRACK_GAIN", "-7.25 dB"),
        ("REPLAYGAIN_ALBUM_GAIN", "-6.00 dB"),
        ("REPLAYGAIN_TRACK_PEAK", "0.5"),
    ] {
        ilst.insert(Atom::new(
            AtomIdent::Freeform {
                mean: "com.apple.iTunes".into(),
                name: name.into(),
            },
            AtomData::UTF8(value.into()),
        ));
    }
    ilst.save_to_path(&m4a, WriteOptions::default()).unwrap();
    // mp3gain's APEv2 tag at the end of an MP3.
    let mp3 = dir.path().join("b.mp3");
    encode_sine(&mp3, &[]);
    let mut ape = ApeTag::default();
    for (key, value) in [
        ("REPLAYGAIN_TRACK_GAIN", "-3.00 dB"),
        ("REPLAYGAIN_ALBUM_GAIN", "-2.00 dB"),
        ("REPLAYGAIN_TRACK_PEAK", "1.023"),
    ] {
        ape.insert(ApeItem::new(key.into(), ItemValue::Text(value.into())).unwrap());
    }
    ape.save_to_path(&mp3, WriteOptions::default()).unwrap();

    let (_, tracks) = item_of(&app, "album", "Album", &[&m4a, &mp3]).await;
    read_tags(&app).await;
    let m4a = row(&app, tracks[0]).await;
    assert_eq!(
        (m4a.gain_db, m4a.album_gain_db, m4a.peak, state(&m4a)),
        (Some(-7.25), Some(-6.0), Some(0.5), Some("done"))
    );
    let mp3 = row(&app, tracks[1]).await;
    assert_eq!(
        (mp3.gain_db, mp3.album_gain_db, mp3.peak, state(&mp3)),
        (Some(-3.0), Some(-2.0), Some(1.023), Some("done"))
    );
}

#[sqlx::test]
async fn a_file_that_is_not_there_is_looked_at_again_later(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaeter.wav");
    let (_, tracks) = item_of(&app, "album", "Album", &[&path]).await;
    // A share that is not mounted: nothing is known, so nothing is decided.
    assert_eq!(read_tags(&app).await.read, 0);
    assert_eq!(state(&row(&app, tracks[0]).await), None);
    loudness::run(&app.state).await;
    assert_eq!(state(&row(&app, tracks[0]).await), None);

    write_sine(&path, 3.0, -20.0);
    loudness::run(&app.state).await;
    let found = row(&app, tracks[0]).await;
    assert_eq!(state(&found), Some("done"));
    assert!(near(found.gain_db, 2.0, 0.2), "{found:?}");
}

// --- measuring ---------------------------------------------------------------------------------

#[sqlx::test]
async fn a_sine_of_known_level_gets_the_expected_gain_and_peak(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sinus.wav");
    write_sine(&path, 5.0, -23.0);
    let (_, tracks) = item_of(&app, "album", "Messung", &[&path]).await;

    loudness::run(&app.state).await;

    let found = row(&app, tracks[0]).await;
    // -23 LUFS measured, -18 wanted: five dB up.
    assert!(near(found.gain_db, 5.0, 0.15), "{found:?}");
    // Alone on its album, the album's value is its own.
    assert!(near(found.album_gain_db, 5.0, 0.15), "{found:?}");
    assert!(
        near(found.peak, 10f64.powf(-23.0 / 20.0), 0.001),
        "{found:?}"
    );
    assert_eq!(state(&found), Some("done"));
    assert_eq!(
        app.state.loudness.snapshot(),
        LoudnessProgress {
            running: false,
            total: 1,
            processed: 1,
            failed: 0
        }
    );
}

#[sqlx::test]
async fn lossy_formats_are_measured_too(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    // About -18 LUFS each (see `encode_sine`), whatever the codec made of it.
    let names = ["a.mp3", "b.flac", "c.m4a", "d.ogg", "e.opus"];
    let mut tracks = Vec::new();
    for name in names {
        let path = dir.path().join(name);
        encode_sine(&path, &[]);
        // An album each, so that each file's own measurement is what is written.
        let (_, ids) = item_of(&app, "album", name, &[&path]).await;
        tracks.push(ids[0]);
    }
    loudness::run(&app.state).await;
    for (name, track) in names.iter().zip(tracks) {
        let found = row(&app, track).await;
        assert_eq!(state(&found), Some("done"), "{name}");
        assert!(near(found.gain_db, 0.0, 0.6), "{name}: {found:?}");
        assert!(near(found.peak, 0.125, 0.03), "{name}: {found:?}");
    }
}

#[sqlx::test]
async fn an_album_gets_one_value_over_all_of_its_tracks(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let loud = dir.path().join("01 laut.wav");
    let softer = dir.path().join("02 leiser.wav");
    write_sine(&loud, 6.0, -10.0);
    write_sine(&softer, 6.0, -16.0);
    let (_, tracks) = item_of(&app, "album", "Dynamik", &[&loud, &softer]).await;

    loudness::run(&app.state).await;

    let (first, second) = (row(&app, tracks[0]).await, row(&app, tracks[1]).await);
    assert!(near(first.gain_db, -8.0, 0.15), "{first:?}");
    assert!(near(second.gain_db, -2.0, 0.15), "{second:?}");
    // Both tracks carry the same album value, so the quiet one stays quieter.
    assert_eq!(first.album_gain_db, second.album_gain_db);
    // The energy of both halves together: 10·log10((10^-1.0 + 10^-1.6) / 2) = -12.03 LUFS —
    // not the -13 a mean of the two values would give.
    assert!(near(first.album_gain_db, -5.97, 0.15), "{first:?}");
}

#[sqlx::test]
async fn tags_outrank_the_measurement(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    // A track gain in the tags but no album gain: the album still has to be measured.
    let tagged = dir.path().join("01.flac");
    encode_sine(
        &tagged,
        &[
            ("REPLAYGAIN_TRACK_GAIN", "-9.99 dB"),
            ("REPLAYGAIN_TRACK_PEAK", "0.9"),
        ],
    );
    let plain = dir.path().join("02.wav");
    write_sine(&plain, 3.0, -18.0);
    let (_, tracks) = item_of(&app, "album", "Halb getaggt", &[&tagged, &plain]).await;

    loudness::run(&app.state).await;

    let tagged = row(&app, tracks[0]).await;
    assert_eq!((tagged.gain_db, tagged.peak), (Some(-9.99), Some(0.9)));
    assert_eq!(state(&tagged), Some("done"));
    let plain = row(&app, tracks[1]).await;
    assert!(near(plain.gain_db, 0.0, 0.15), "{plain:?}");
    // Both sines are at about -18 LUFS, and so is the album.
    assert!(near(tagged.album_gain_db, 0.0, 0.5), "{tagged:?}");
    assert_eq!(tagged.album_gain_db, plain.album_gain_db);
}

#[sqlx::test]
async fn spoken_word_is_not_measured(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let chapter = dir.path().join("kapitel.wav");
    write_sine(&chapter, 3.0, -20.0);
    let tagged = dir.path().join("getaggt.flac");
    encode_sine(&tagged, &[("REPLAYGAIN_TRACK_GAIN", "-4.00 dB")]);
    let (_, book) = item_of(&app, "book", "Hörbuch", &[&chapter, &tagged]).await;

    loudness::run(&app.state).await;

    let plain = row(&app, book[0]).await;
    assert_eq!(
        plain,
        Row {
            gain_db: None,
            album_gain_db: None,
            peak: None,
            loudness_state: Some("done".into()),
        }
    );
    // What the tags say costs nothing and is kept.
    assert_eq!(row(&app, book[1]).await.gain_db, Some(-4.0));
    assert_eq!(app.state.loudness.snapshot().total, 0);
}

// --- the pass ----------------------------------------------------------------------------------

#[sqlx::test]
async fn a_later_pass_carries_on_instead_of_starting_over(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let mut albums = Vec::new();
    for name in ["eins", "zwei", "drei"] {
        let path = dir.path().join(format!("{name}.wav"));
        write_sine(&path, 3.0, -20.0);
        albums.push(item_of(&app, "album", name, &[&path]).await);
    }
    loudness::run(&app.state).await;
    assert_eq!(app.state.loudness.snapshot().processed, 3);

    // As after a restart in the middle: one album is still open, two are through. Their
    // values are marked so that measuring them again would show.
    sqlx::query("UPDATE tracks SET gain_db = 99, album_gain_db = 99 WHERE item_id <> $1")
        .bind(albums[2].0)
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE tracks SET gain_db = NULL, album_gain_db = NULL, peak = NULL,
             loudness_state = 'pending' WHERE item_id = $1",
    )
    .bind(albums[2].0)
    .execute(app.db())
    .await
    .unwrap();

    // A fresh process: new state on the same database.
    let restarted = TestApp::new(app.db().clone());
    loudness::run(&restarted.state).await;
    let progress = restarted.state.loudness.snapshot();
    assert_eq!((progress.total, progress.processed), (1, 1));
    assert_eq!(row(&app, albums[0].1[0]).await.gain_db, Some(99.0));
    assert_eq!(row(&app, albums[1].1[0]).await.gain_db, Some(99.0));
    assert!(near(row(&app, albums[2].1[0]).await.gain_db, 2.0, 0.15));

    // Nothing left: the next pass finds no work at all.
    loudness::run(&restarted.state).await;
    assert_eq!(restarted.state.loudness.snapshot().total, 0);
}

#[sqlx::test]
async fn a_broken_file_does_not_stop_the_pass_and_is_not_tried_again(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("01 kaputt.mp3");
    std::fs::write(
        &broken,
        b"das ist keine Musik, nur Text mit der falschen Endung",
    )
    .unwrap();
    let good = dir.path().join("02 gut.wav");
    write_sine(&good, 3.0, -20.0);
    let (_, tracks) = item_of(&app, "album", "Gemischt", &[&broken, &good]).await;
    let other = dir.path().join("anderes.wav");
    write_sine(&other, 3.0, -12.0);
    let (_, other_tracks) = item_of(&app, "album", "Anderes", &[&other]).await;

    loudness::run(&app.state).await;

    let progress = app.state.loudness.snapshot();
    assert_eq!(
        (progress.total, progress.processed, progress.failed),
        (2, 2, 1)
    );
    let failed = row(&app, tracks[0]).await;
    assert_eq!((failed.gain_db, failed.peak), (None, None));
    assert_eq!(state(&failed), Some("failed"));
    let good = row(&app, tracks[1]).await;
    assert!(near(good.gain_db, 2.0, 0.15), "{good:?}");
    // The album's value comes from what could be measured, and every track carries it.
    assert!(near(good.album_gain_db, 2.0, 0.15), "{good:?}");
    assert_eq!(failed.album_gain_db, good.album_gain_db);
    assert!(near(row(&app, other_tracks[0]).await.gain_db, -6.0, 0.15));

    // Remembered: the next pass has nothing to do.
    loudness::run(&app.state).await;
    assert_eq!(app.state.loudness.snapshot().total, 0);
    assert_eq!(state(&row(&app, tracks[0]).await), Some("failed"));
}

#[sqlx::test]
async fn switched_off_only_the_tags_are_read(db: PgPool) {
    let app = TestApp::new(db);
    app.state.settings.update(|s| s.loudness = false);
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("01.wav");
    write_sine(&plain, 3.0, -20.0);
    let tagged = dir.path().join("02.flac");
    encode_sine(
        &tagged,
        &[
            ("REPLAYGAIN_TRACK_GAIN", "-6.00 dB"),
            ("REPLAYGAIN_ALBUM_GAIN", "-5.00 dB"),
        ],
    );
    let (_, tracks) = item_of(&app, "album", "Album", &[&plain, &tagged]).await;

    loudness::run(&app.state).await;

    let plain = row(&app, tracks[0]).await;
    assert_eq!((plain.gain_db, state(&plain)), (None, Some("pending")));
    let tagged = row(&app, tracks[1]).await;
    assert_eq!(
        (tagged.gain_db, tagged.album_gain_db),
        (Some(-6.0), Some(-5.0))
    );
    assert_eq!(app.state.loudness.snapshot(), LoudnessProgress::default());

    // Switched on again later, the open ones are measured.
    let on = TestApp::new(app.db().clone());
    loudness::run(&on.state).await;
    assert!(near(row(&app, tracks[0]).await.gain_db, 2.0, 0.15));
}

#[sqlx::test]
async fn a_stopped_job_measures_nothing(db: PgPool) {
    let app = TestApp::new(db);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("01.wav");
    write_sine(&path, 3.0, -20.0);
    let (_, tracks) = item_of(&app, "album", "Album", &[&path]).await;
    read_tags(&app).await;

    // Shutdown: what is open stays open for the next start.
    app.state.loudness.stop();
    loudness::measure_pending(&app.state).await.unwrap();
    loudness::run(&app.state).await;
    let found = row(&app, tracks[0]).await;
    assert_eq!((found.gain_db, state(&found)), (None, Some("pending")));
    assert!(!app.state.loudness.snapshot().running);
}

// --- with the scan -----------------------------------------------------------------------------

async fn wait_for_loudness(app: &TestApp) {
    for _ in 0..600 {
        if !app.state.loudness.snapshot().running {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("loudness pass did not finish within 30 s");
}

async fn scan(app: &TestApp, token: &str) {
    let response = app.post_raw("/scan", "", Some(token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let status = app.wait_for_scan().await;
    assert_eq!(status.last_error, None);
    wait_for_loudness(app).await;
}

async fn track_by_path(app: &TestApp, path: &Path) -> i32 {
    sqlx::query_scalar("SELECT id FROM tracks WHERE path = $1")
        .bind(path.to_str().unwrap())
        .fetch_one(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn a_scan_reads_the_tags_and_the_pass_after_it_measures(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let admin = app.user("admin", true).await;
    let token = app.token(admin).await;
    set_library_paths(
        app.db(),
        &TempLibrary::path_str(&library.books()),
        &TempLibrary::path_str(&library.music()),
    )
    .await
    .unwrap();
    let album = library.music().join("Band").join("Platte");
    let tagged = album.join("01 getaggt.flac");
    encode_sine(
        &tagged,
        &[
            ("REPLAYGAIN_TRACK_GAIN", "-6.00 dB"),
            ("REPLAYGAIN_ALBUM_GAIN", "-5.00 dB"),
            ("REPLAYGAIN_TRACK_PEAK", "0.8"),
        ],
    );
    let other = library.music().join("Band").join("Ohne Tags");
    let plain = other.join("01 ohne.wav");
    write_sine(&plain, 3.0, -20.0);
    // Stands in for a file that cannot be decoded, further down.
    let broken = other.join("02 kaputt.wav");
    write_sine(&broken, 3.0, -20.0);

    scan(&app, &token).await;

    let tagged_id = track_by_path(&app, &tagged).await;
    let found = row(&app, tagged_id).await;
    assert_eq!(
        (found.gain_db, found.album_gain_db, found.peak),
        (Some(-6.0), Some(-5.0), Some(0.8))
    );
    let plain_id = track_by_path(&app, &plain).await;
    assert!(near(row(&app, plain_id).await.gain_db, 2.0, 0.15));

    // The status says how the pass went.
    let status = body_json(app.get("/scan/status", Some(&token)).await).await;
    assert_eq!(
        status["loudness"],
        json!({"running": false, "total": 1, "processed": 1, "failed": 0})
    );

    // A file that could not be decoded is left alone for as long as it stays the same …
    let broken_id = track_by_path(&app, &broken).await;
    assert!(row(&app, broken_id).await.gain_db.is_some());
    sqlx::query(
        "UPDATE tracks SET gain_db = NULL, peak = NULL, loudness_state = 'failed' WHERE id = $1",
    )
    .bind(broken_id)
    .execute(app.db())
    .await
    .unwrap();
    scan(&app, &token).await;
    let found = row(&app, broken_id).await;
    assert_eq!((found.gain_db, state(&found)), (None, Some("failed")));
    assert_eq!(app.state.loudness.snapshot().total, 0);
    // … and done again once it changes (another size, another mtime).
    write_sine(&broken, 4.0, -14.0);
    scan(&app, &token).await;
    let again = row(&app, track_by_path(&app, &broken).await).await;
    assert_eq!(state(&again), Some("done"));
    assert!(near(again.gain_db, -4.0, 0.15), "{again:?}");
    // The untouched neighbour was measured along (the album's value changed) and kept its own.
    let neighbour = row(&app, plain_id).await;
    assert!(near(neighbour.gain_db, 2.0, 0.15));
    assert_eq!(neighbour.album_gain_db, again.album_gain_db);
}

#[sqlx::test]
async fn an_album_that_loses_a_track_is_measured_again(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let admin = app.user("admin", true).await;
    let token = app.token(admin).await;
    set_library_paths(
        app.db(),
        &TempLibrary::path_str(&library.books()),
        &TempLibrary::path_str(&library.music()),
    )
    .await
    .unwrap();
    let album = library.music().join("Band").join("Platte");
    let quiet = album.join("01 leise.wav");
    write_sine(&quiet, 3.0, -20.0);
    let loud = album.join("02 laut.wav");
    write_sine(&loud, 3.0, -8.0);
    scan(&app, &token).await;
    let quiet_id = track_by_path(&app, &quiet).await;
    let before = row(&app, quiet_id).await;
    // The loud track pulls the album's value well below the quiet track's own.
    assert!(
        before.album_gain_db.unwrap() < before.gain_db.unwrap() - 3.0,
        "{before:?}"
    );

    std::fs::remove_file(&loud).unwrap();
    scan(&app, &token).await;

    let after = row(&app, quiet_id).await;
    assert_eq!(state(&after), Some("done"));
    assert!(near(after.gain_db, 2.0, 0.15), "{after:?}");
    assert!(near(after.album_gain_db, 2.0, 0.15), "{after:?}");
}

#[sqlx::test]
async fn an_album_that_takes_in_a_measured_track_is_measured_again(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let admin = app.user("admin", true).await;
    let token = app.token(admin).await;
    set_library_paths(
        app.db(),
        &TempLibrary::path_str(&library.books()),
        &TempLibrary::path_str(&library.music()),
    )
    .await
    .unwrap();
    let album = library.music().join("Band").join("Platte");
    let quiet = album.join("01 leise.wav");
    write_sine(&quiet, 3.0, -20.0);
    scan(&app, &token).await;
    let quiet_id = track_by_path(&app, &quiet).await;
    assert!(near(row(&app, quiet_id).await.album_gain_db, 2.0, 0.15));

    // A loud track with tags that leave nothing to measure joins: the album changed all the same.
    encode_sine(
        &album.join("02 laut.flac"),
        &[
            ("REPLAYGAIN_TRACK_GAIN", "-6.00 dB"),
            ("REPLAYGAIN_ALBUM_GAIN", "-6.00 dB"),
        ],
    );
    scan(&app, &token).await;
    let after = row(&app, quiet_id).await;
    assert!(near(after.gain_db, 2.0, 0.15), "{after:?}");
    assert!(after.album_gain_db.unwrap() < 1.0, "{after:?}");
}

// --- the API -----------------------------------------------------------------------------------

async fn get(app: &TestApp, token: &str, uri: &str) -> Value {
    let response = app.get(uri, Some(token)).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_json(response).await
}

fn assert_loudness(track: &Value, gain: Value, album: Value, peak: Value, context: &str) {
    assert_eq!(track["gain_db"], gain, "{context}: {track}");
    assert_eq!(track["album_gain_db"], album, "{context}: {track}");
    assert_eq!(track["peak"], peak, "{context}: {track}");
    // Present even when unknown, as `null`.
    for key in ["gain_db", "album_gain_db", "peak"] {
        assert!(track.get(key).is_some(), "{context}: {key} fehlt");
    }
}

#[sqlx::test]
async fn every_track_shape_carries_the_three_fields(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Die Band"),
            ..Seed::new("album", "Platte")
        },
    )
    .await;
    let known = seed_track(app.db(), album, 1, "/m/1.mp3", 200.0).await;
    let unknown = seed_track(app.db(), album, 2, "/m/2.mp3", 200.0).await;
    sqlx::query("UPDATE tracks SET title = 'Lautlied', gain_db = -7.25, album_gain_db = -6, peak = 0.988553 WHERE id = $1")
        .bind(known)
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query("UPDATE tracks SET title = 'Stilllied' WHERE id = $1")
        .bind(unknown)
        .execute(app.db())
        .await
        .unwrap();
    let expect = |track: &Value, context: &str| {
        assert_loudness(track, json!(-7.25), json!(-6), json!(0.988553), context);
    };

    // TrackOut: the item and the play response.
    let item = get(&app, &token, &format!("/items/{album}")).await;
    expect(&item["tracks"][0], "/items");
    assert_loudness(
        &item["tracks"][1],
        Value::Null,
        Value::Null,
        Value::Null,
        "/items",
    );
    let play = body_json(
        app.post_raw(&format!("/play/{album}"), "", Some(&token))
            .await,
    )
    .await;
    expect(&play["tracks"][0], "/play");
    assert_loudness(
        &play["tracks"][1],
        Value::Null,
        Value::Null,
        Value::Null,
        "/play",
    );
    let single = get(&app, &token, &format!("/tracks/{known}")).await;
    expect(&single, "/tracks/{id}");

    // TrackSummary: search …
    let search = get(&app, &token, "/search?q=Lautlied").await;
    expect(&search["tracks"][0], "/search");
    let search = get(&app, &token, "/search?q=Stilllied").await;
    assert_loudness(
        &search["tracks"][0],
        Value::Null,
        Value::Null,
        Value::Null,
        "/search",
    );

    // … favourites …
    let response = app
        .call(
            Method::POST,
            &format!("/favorites/tracks/{known}"),
            None,
            Some(&token),
        )
        .await;
    assert!(response.status().is_success());
    let favorites = get(&app, &token, "/favorites").await;
    expect(&favorites["tracks"][0], "/favorites");

    // … playlists …
    let created = body_json(
        app.post_json("/playlists", json!({"name": "Liste"}), Some(&token))
            .await,
    )
    .await;
    let playlist = created["id"].as_i64().unwrap();
    app.post_json(
        &format!("/playlists/{playlist}/entries"),
        json!({"track_id": known}),
        Some(&token),
    )
    .await;
    let detail = get(&app, &token, &format!("/playlists/{playlist}")).await;
    expect(&detail["entries"][0]["track"], "/playlists/{id}");

    // … the Titel-Radio (it plays other albums than the seed's, so there is a second one) …
    let second = seed(
        app.db(),
        Seed {
            artist: Some("Die Band"),
            ..Seed::new("album", "Zweite")
        },
    )
    .await;
    let radio_track = seed_track(app.db(), second, 1, "/m/3.mp3", 200.0).await;
    sqlx::query(
        "UPDATE tracks SET gain_db = -7.25, album_gain_db = -6, peak = 0.988553 WHERE id = $1",
    )
    .bind(radio_track)
    .execute(app.db())
    .await
    .unwrap();
    let radio = get(&app, &token, &format!("/radio?track_id={known}")).await;
    let radio_tracks = radio["tracks"].as_array().unwrap();
    assert!(!radio_tracks.is_empty());
    for track in radio_tracks {
        if track["id"] == json!(unknown) {
            continue;
        }
        expect(track, "/radio");
    }

    // … and the top lists.
    for ago in ["1 day", "2 days", "3 days"] {
        sqlx::query(
            "INSERT INTO listen_events (user_id, item_id, track_id, seconds, at)
             VALUES ($1, $2, $3, 200, now() - $4::text::interval)",
        )
        .bind(user)
        .bind(album)
        .bind(known)
        .bind(ago)
        .execute(app.db())
        .await
        .unwrap();
    }
    let repeat = get(&app, &token, "/me/on-repeat").await;
    expect(&repeat["tracks"][0], "/me/on-repeat");
    let top = get(&app, &token, "/me/top?kind=tracks").await;
    expect(&top["entries"][0]["track"], "/me/top");
}
