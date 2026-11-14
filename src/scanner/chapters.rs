//! Chapter marks embedded in a single-file audiobook, read with `ffprobe` (legacy
//! `scanner/chapters.ts`). Never fails: a missing ffprobe, a timeout or odd output all give `[]`.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedChapter {
    pub title: String,
    pub start: f64,
    pub end: f64,
}

const TIMEOUT: Duration = Duration::from_secs(30);

pub fn read_chapters(path: &Path) -> Vec<ScannedChapter> {
    run_ffprobe(path)
        .and_then(|stdout| parse_chapters(&stdout))
        .unwrap_or_default()
}

fn run_ffprobe(path: &Path) -> Option<Vec<u8>> {
    let mut child = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_chapters"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drained on its own thread, so a long chapter list cannot fill the pipe and stall ffprobe
    // while this thread waits for it to exit.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        stdout.read_to_end(&mut buffer).map(|_| buffer)
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let output = reader.join().ok()?.ok()?;
    status.success().then_some(output)
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    chapters: Vec<ProbeChapter>,
}

#[derive(Deserialize)]
struct ProbeChapter {
    start_time: serde_json::Value,
    end_time: serde_json::Value,
    #[serde(default)]
    tags: Option<ProbeTags>,
}

#[derive(Deserialize)]
struct ProbeTags {
    title: Option<String>,
}

/// ffprobe prints times as strings ("12.345000"); `Number()` in legacy took numbers too.
fn seconds(value: &serde_json::Value) -> Option<f64> {
    let n = match value {
        serde_json::Value::Number(n) => n.as_f64()?,
        serde_json::Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

/// One malformed chapter discards them all — a partial list would misplace every mark after it.
pub fn parse_chapters(stdout: &[u8]) -> Option<Vec<ScannedChapter>> {
    let probe: Probe = serde_json::from_slice(stdout).ok()?;
    probe
        .chapters
        .iter()
        .enumerate()
        .map(|(i, chapter)| {
            let title = chapter
                .tags
                .as_ref()
                .and_then(|t| t.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("Kapitel {}", i + 1));
            Some(ScannedChapter {
                title,
                start: seconds(&chapter.start_time)?,
                end: seconds(&chapter.end_time)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_output_and_numbers_untitled_chapters() {
        let json = br#"{"chapters":[
            {"start_time":"0.000000","end_time":"60.5","tags":{"title":"Prolog"}},
            {"start_time":"60.5","end_time":"120.0"}
        ]}"#;
        assert_eq!(
            parse_chapters(json),
            Some(vec![
                ScannedChapter {
                    title: "Prolog".into(),
                    start: 0.0,
                    end: 60.5
                },
                ScannedChapter {
                    title: "Kapitel 2".into(),
                    start: 60.5,
                    end: 120.0
                },
            ])
        );
    }

    #[test]
    fn a_malformed_chapter_discards_the_list() {
        let json = br#"{"chapters":[{"start_time":"x","end_time":"1"}]}"#;
        assert_eq!(parse_chapters(json), None);
        assert_eq!(parse_chapters(b"{}"), Some(vec![]));
        assert_eq!(parse_chapters(b"nonsense"), None);
    }

    #[test]
    fn never_fails_for_a_file_that_does_not_exist() {
        assert_eq!(read_chapters(Path::new("/nonexistent/file.m4b")), vec![]);
    }
}
