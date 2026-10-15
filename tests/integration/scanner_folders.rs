//! Walking real folders with real audio files (legacy tests/integration/scanner-folders.test.ts,
//! tests/unit/tags.test.ts, tests/unit/chapters.test.ts). Needs ffmpeg/ffprobe; no database.

use std::path::Path;

use autoreverse_server::scanner::books::{ItemKind, KnownFiles, ScannedItem, scan_tree};
use autoreverse_server::scanner::chapters::read_chapters;
use autoreverse_server::scanner::tags::read_tags;

use crate::common::{make_audio, make_audio_with};

fn scan(root: &Path, known: &KnownFiles) -> Vec<ScannedItem> {
    scan_tree(
        root,
        ItemKind::Book,
        known,
        &mut Vec::new(),
        |_, _| {},
        || false,
    )
}

fn known_entry(path: &Path) -> (String, (f64, i64)) {
    let metadata = std::fs::metadata(path).unwrap();
    let mtime = metadata
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    (
        path.to_str().unwrap().to_owned(),
        (mtime, metadata.len() as i64),
    )
}

#[test]
fn derives_author_and_series_from_author_series_book_folders() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Andreas Eschbach/Solarstation/Buch 1");
    make_audio(&book.join("01.mp3"), &[("title", "Teil 1")]);
    make_audio(&book.join("02.mp3"), &[("title", "Teil 2")]);
    let scanned = scan(root.path(), &KnownFiles::new());
    assert_eq!(scanned.len(), 1);
    let item = &scanned[0];
    assert_eq!(item.author.as_deref(), Some("Andreas Eschbach"));
    assert_eq!(item.series.as_deref(), Some("Solarstation"));
    assert_eq!(item.tracks.len(), 2);
    assert!(!item.unchanged);
}

#[test]
fn an_audiobook_in_disc_folders_is_one_book_in_its_series_with_chapters_across_discs() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Andreas Eschbach/Solarstation/Buch 1");
    for (disc, title) in [
        ("CD 2", "Kapitel 3"),
        ("CD 1", "Kapitel 1"),
        ("CD 1", "Kapitel 2"),
    ] {
        let file = format!("{}.mp3", title.replace(' ', "_"));
        make_audio(
            &book.join(disc).join(file),
            &[
                ("title", title),
                ("album", &format!("Solarstation ({disc})")),
            ],
        );
    }
    let scanned = scan(root.path(), &KnownFiles::new());
    assert_eq!(scanned.len(), 1);
    let item = &scanned[0];
    assert_eq!(item.source_path, book.to_str().unwrap());
    assert_eq!(item.author.as_deref(), Some("Andreas Eschbach"));
    assert_eq!(item.series.as_deref(), Some("Solarstation"));
    let chapters: Vec<&str> = item.chapters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(chapters, ["Kapitel 1", "Kapitel 2", "Kapitel 3"]);
    let discs: Vec<Option<i32>> = item.tracks.iter().map(|t| t.disc).collect();
    assert_eq!(discs, [Some(1), Some(1), Some(2)]);
}

#[test]
fn synthesizes_one_chapter_per_track_for_more_than_one_file() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("A/B");
    make_audio(&book.join("01.mp3"), &[("title", "Erster Teil")]);
    make_audio(&book.join("02.mp3"), &[("title", "Zweiter Teil")]);
    let item = &scan(root.path(), &KnownFiles::new())[0];
    assert_eq!(item.chapters.len(), 2);
    assert_eq!(item.chapters[0].title, "Erster Teil");
    assert_eq!(item.chapters[0].start, 0.0);
    assert!((item.chapters[1].start - item.tracks[0].duration).abs() < 1e-6);
}

#[test]
fn marks_a_folder_unchanged_when_the_known_map_matches_exactly() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("A/B/01.mp3");
    make_audio(&file, &[]);
    let known: KnownFiles = [known_entry(&file)].into();
    let item = &scan(root.path(), &known)[0];
    assert!(item.unchanged);
    assert!(item.tracks.is_empty());
    assert_eq!(item.title, "B");
    assert_eq!(item.author.as_deref(), Some("A"));
}

#[test]
fn skips_a_folder_without_audio_but_descends_into_it() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Empty/HasAudio");
    make_audio(&book.join("01.mp3"), &[]);
    std::fs::write(root.path().join("Empty/notes.txt"), "x").unwrap();
    let scanned = scan(root.path(), &KnownFiles::new());
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].source_path, book.to_str().unwrap());
}

#[test]
fn detects_a_file_removed_from_disk() {
    let root = tempfile::tempdir().unwrap();
    let (one, two) = (
        root.path().join("A/B/01.mp3"),
        root.path().join("A/B/02.mp3"),
    );
    make_audio(&one, &[]);
    make_audio(&two, &[]);
    let known: KnownFiles = [known_entry(&one), known_entry(&two)].into();
    std::fs::remove_file(&two).unwrap();
    let item = &scan(root.path(), &known)[0];
    assert!(!item.unchanged, "a removed file must count as a change");
    assert_eq!(item.tracks.len(), 1);
    assert_eq!(item.tracks[0].path, one.to_str().unwrap());
}

#[test]
fn does_not_count_files_of_sibling_folders_as_its_own() {
    let root = tempfile::tempdir().unwrap();
    let files: Vec<_> = [
        "A1/B1/01.mp3",
        "A1/B1/02.mp3",
        "A2/B2/01.mp3",
        "A2/B2/02.mp3",
    ]
    .iter()
    .map(|f| root.path().join(f))
    .collect();
    for file in &files {
        make_audio(file, &[]);
    }
    let known: KnownFiles = files.iter().map(|f| known_entry(f)).collect();
    let folder = root.path().join("A1/B1");
    let scanned = scan(&folder, &known);
    assert_eq!(scanned.len(), 1);
    assert!(scanned[0].unchanged);
    assert_eq!(scanned[0].source_path, folder.to_str().unwrap());
}

#[test]
fn takes_title_and_series_from_the_tags() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Autor/Ordner");
    make_audio(
        &book.join("01.mp3"),
        &[
            ("album", "Perry Rhodan, Band 3 - Der Fluch"),
            ("artist", "Tag-Autor"),
            ("comment", "Ein Klappentext."),
        ],
    );
    let item = &scan(root.path(), &KnownFiles::new())[0];
    assert_eq!(item.title, "Der Fluch");
    assert_eq!(item.series.as_deref(), Some("Perry Rhodan"));
    assert_eq!(item.series_index, Some(3.0));
    assert_eq!(item.author.as_deref(), Some("Tag-Autor"));
    assert_eq!(item.description.as_deref(), Some("Ein Klappentext."));
}

#[test]
fn reads_a_series_from_a_free_form_frame() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    make_audio(&file, &[("SERIES", "Die drei ???"), ("SERIES-PART", "42")]);
    let tags = read_tags(&file);
    assert_eq!(tags.series.as_deref(), Some("Die drei ???"));
    assert_eq!(tags.series_index, Some(42.0));
}

#[test]
fn reads_title_artist_album_track_and_year() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    make_audio(
        &file,
        &[
            ("title", "Erste Bahn"),
            ("artist", "Ansa Volt"),
            ("album", "Nordlicht"),
            ("track", "1"),
            ("date", "2022"),
        ],
    );
    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Erste Bahn"));
    assert_eq!(tags.artist.as_deref(), Some("Ansa Volt"));
    assert_eq!(tags.album.as_deref(), Some("Nordlicht"));
    assert_eq!(tags.track, Some(1));
    assert_eq!(tags.year, Some(2022));
    assert!(tags.readable);
    assert!(tags.duration > 0.0);
}

#[test]
fn a_file_that_is_not_audio_is_unreadable_never_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-audio.mp3");
    std::fs::write(&file, "not actually audio").unwrap();
    let tags = read_tags(&file);
    assert!(!tags.readable);
    assert_eq!(tags.title, None);
    assert_eq!(tags.duration, 0.0);
}

#[test]
fn a_file_without_chapter_marks_has_no_chapters() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    make_audio(&file, &[]);
    assert!(read_chapters(&file).is_empty());
}

#[test]
fn reads_the_chapter_marks_of_a_single_file_book() {
    let dir = tempfile::tempdir().unwrap();
    let marks = dir.path().join("chapters.txt");
    std::fs::write(
        &marks,
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=400\ntitle=Prolog\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=400\nEND=1000\ntitle=Das Ende\n",
    )
    .unwrap();
    let book = dir.path().join("Autor/Ein Buch");
    let file = book.join("buch.m4b");
    make_audio_with(
        &file,
        &[],
        &[
            "-i",
            marks.to_str().unwrap(),
            "-map_metadata",
            "1",
            "-c:a",
            "aac",
        ],
    );
    let chapters = read_chapters(&file);
    let titles: Vec<_> = chapters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Prolog", "Das Ende"]);
    assert!((chapters[1].start - 0.4).abs() < 0.01);

    // The scan uses them instead of one chapter per file.
    let item = &scan(dir.path(), &KnownFiles::new())
        .into_iter()
        .find(|i| i.source_path == book.to_str().unwrap())
        .unwrap();
    assert_eq!(item.chapters.len(), 2);
    assert_eq!(item.chapters[0].title, "Prolog");
}
