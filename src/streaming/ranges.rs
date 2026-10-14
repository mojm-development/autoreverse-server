//! HTTP range requests over a file on disk (legacy `streaming/ranges.ts`).
//!
//! Deliberately as small as legacy: one range per request (everything after a comma is ignored,
//! players never ask for more), no `If-Range`, no validators. A header that does not parse is not
//! an error — the whole file goes out with 200, which every player copes with.

use std::path::Path;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

/// Chunk size of the response body.
const BLOCK_SIZE: usize = 64 * 1024;

/// The `Content-Type` for an audio file, by extension.
pub fn media_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("m4a" | "m4b") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("ogg" | "opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("aac") => "audio/aac",
        _ => "application/octet-stream",
    }
}

/// An inclusive byte range within the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

/// The range lies outside the file: 416.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeNotSatisfiable;

fn digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // A run of digits too long for u64 is still a number, just a huge one (JS `Number`).
    Some(text.parse().unwrap_or(u64::MAX))
}

/// The first range of a `Range` header. `Ok(None)` means "ignore the header and send it all".
pub fn parse_range(header: Option<&str>, size: u64) -> Result<Option<Span>, RangeNotSatisfiable> {
    let Some(spec) = header.and_then(|h| h.strip_prefix("bytes=")) else {
        return Ok(None);
    };
    let part = spec.split(',').next().unwrap_or("").trim();
    let Some((start_text, end_text)) = part.split_once('-') else {
        return Ok(None);
    };

    if start_text.is_empty() {
        // Suffix range: the last N bytes.
        let Some(length) = digits(end_text) else {
            return Ok(None);
        };
        // Legacy answered an empty file with the nonsense range `0--1`; there is nothing to
        // send, so it is unsatisfiable like any other range into an empty file.
        if length == 0 || size == 0 {
            return Err(RangeNotSatisfiable);
        }
        return Ok(Some(Span {
            start: size.saturating_sub(length),
            end: size - 1,
        }));
    }

    let Some(start) = digits(start_text) else {
        return Ok(None);
    };
    if start >= size {
        return Err(RangeNotSatisfiable);
    }
    let end = digits(end_text).unwrap_or(size - 1).min(size - 1);
    if end < start {
        return Err(RangeNotSatisfiable);
    }
    Ok(Some(Span { start, end }))
}

fn header_value(value: String) -> HeaderValue {
    HeaderValue::try_from(value).expect("ASCII header value")
}

/// The file, whole (200) or the requested slice (206), or 416. `head` sends the headers only.
pub async fn range_response(
    path: &Path,
    range: Option<&str>,
    head: bool,
) -> std::io::Result<Response> {
    let size = tokio::fs::metadata(path).await?.len();
    let content_type = HeaderValue::from_static(media_type(path));

    let span = match parse_range(range, size) {
        Ok(span) => span,
        Err(RangeNotSatisfiable) => {
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(
                    header::CONTENT_RANGE,
                    header_value(format!("bytes */{size}")),
                )],
            )
                .into_response());
        }
    };

    let (status, start, length) = match span {
        None => (StatusCode::OK, 0, size),
        Some(Span { start, end }) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
    };

    let body = if head {
        Body::empty()
    } else {
        let mut file = tokio::fs::File::open(path).await?;
        if start > 0 {
            file.seek(std::io::SeekFrom::Start(start)).await?;
        }
        Body::from_stream(ReaderStream::with_capacity(file.take(length), BLOCK_SIZE))
    };

    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    if let Some(Span { start, end }) = span {
        headers.insert(
            header::CONTENT_RANGE,
            header_value(format!("bytes {start}-{end}/{size}")),
        );
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    const SIZE: u64 = 1000;

    fn span(start: u64, end: u64) -> Result<Option<Span>, RangeNotSatisfiable> {
        Ok(Some(Span { start, end }))
    }

    #[test]
    fn no_header_or_an_unparseable_one_means_the_whole_file() {
        assert_eq!(parse_range(None, SIZE), Ok(None));
        assert_eq!(parse_range(Some("bytes = 0-99"), SIZE), Ok(None));
        assert_eq!(parse_range(Some("nonsense"), SIZE), Ok(None));
        assert_eq!(parse_range(Some("bytes=abc-"), SIZE), Ok(None));
        assert_eq!(parse_range(Some("bytes=-x"), SIZE), Ok(None));
        assert_eq!(parse_range(Some("bytes=12"), SIZE), Ok(None));
    }

    #[test]
    fn parses_a_normal_range() {
        assert_eq!(parse_range(Some("bytes=0-99"), SIZE), span(0, 99));
    }

    #[test]
    fn caps_open_ended_and_oversized_ranges() {
        assert_eq!(parse_range(Some("bytes=500-"), SIZE), span(500, 999));
        assert_eq!(parse_range(Some("bytes=0-99999"), SIZE), span(0, 999));
        assert_eq!(
            parse_range(Some("bytes=0-99999999999999999999999"), SIZE),
            span(0, 999)
        );
    }

    #[test]
    fn parses_a_suffix_range() {
        assert_eq!(parse_range(Some("bytes=-500"), SIZE), span(500, 999));
        assert_eq!(parse_range(Some("bytes=-5000"), SIZE), span(0, 999));
    }

    #[test]
    fn unsatisfiable_ranges() {
        for header in ["bytes=1000-", "bytes=5-2", "bytes=-0", "bytes=1000-1999"] {
            assert_eq!(
                parse_range(Some(header), SIZE),
                Err(RangeNotSatisfiable),
                "{header}"
            );
        }
        assert_eq!(parse_range(Some("bytes=-10"), 0), Err(RangeNotSatisfiable));
    }

    #[test]
    fn only_the_first_of_several_ranges_counts() {
        assert_eq!(parse_range(Some("bytes=0-99,200-299"), SIZE), span(0, 99));
        assert_eq!(
            parse_range(Some("bytes= 10-19 ,200-299"), SIZE),
            span(10, 19)
        );
    }

    #[test]
    fn media_types_by_extension() {
        let t = |p: &str| media_type(Path::new(p));
        assert_eq!(t("/x/a.mp3"), "audio/mpeg");
        assert_eq!(t("/x/a.m4b"), "audio/mp4");
        assert_eq!(t("/x/a.m4a"), "audio/mp4");
        assert_eq!(t("/x/a.flac"), "audio/flac");
        assert_eq!(t("/x/a.ogg"), "audio/ogg");
        assert_eq!(t("/x/a.opus"), "audio/ogg");
        assert_eq!(t("/x/a.wav"), "audio/wav");
        assert_eq!(t("/x/a.aac"), "audio/aac");
        assert_eq!(t("/x/A.MP3"), "audio/mpeg");
        assert_eq!(t("/x/a.xyz"), "application/octet-stream");
        assert_eq!(t("/x/noext"), "application/octet-stream");
    }

    /// A 1000-byte file whose byte `i` is `i % 256`.
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.mp3");
        let content: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    async fn body(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    fn h(response: &Response, name: header::HeaderName) -> Option<&str> {
        response.headers().get(name).map(|v| v.to_str().unwrap())
    }

    #[tokio::test]
    async fn without_a_range_the_whole_file_goes_out_with_200() {
        let (_dir, path) = fixture();
        let response = range_response(&path, None, false).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(h(&response, header::CONTENT_TYPE), Some("audio/mpeg"));
        assert_eq!(h(&response, header::CONTENT_LENGTH), Some("1000"));
        assert_eq!(h(&response, header::ACCEPT_RANGES), Some("bytes"));
        assert_eq!(h(&response, header::CONTENT_RANGE), None);
        let bytes = body(response).await;
        assert_eq!(bytes.len(), 1000);
        assert!(bytes.iter().enumerate().all(|(i, b)| *b == (i % 256) as u8));
    }

    #[tokio::test]
    async fn head_sends_the_same_headers_without_a_body() {
        let (_dir, path) = fixture();
        let response = range_response(&path, None, true).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(h(&response, header::CONTENT_LENGTH), Some("1000"));
        assert_eq!(h(&response, header::ACCEPT_RANGES), Some("bytes"));
        assert!(body(response).await.is_empty());

        let response = range_response(&path, Some("bytes=100-199"), true)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(h(&response, header::CONTENT_TYPE), Some("audio/mpeg"));
        assert_eq!(
            h(&response, header::CONTENT_RANGE),
            Some("bytes 100-199/1000")
        );
        assert_eq!(h(&response, header::CONTENT_LENGTH), Some("100"));
        assert!(body(response).await.is_empty());
    }

    #[tokio::test]
    async fn malformed_ranges_fall_back_to_the_whole_file() {
        let (_dir, path) = fixture();
        for header in ["bytes = 0-99", "garbage"] {
            let response = range_response(&path, Some(header), false).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{header}");
            assert_eq!(h(&response, header::ACCEPT_RANGES), Some("bytes"));
            assert_eq!(body(response).await.len(), 1000);
        }
        let response = range_response(&path, Some("bytes = 0-99"), true)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body(response).await.is_empty());
    }

    #[tokio::test]
    async fn an_unsatisfiable_range_is_416_with_the_size() {
        let (_dir, path) = fixture();
        for head in [false, true] {
            let response = range_response(&path, Some("bytes=1000-1999"), head)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
            assert_eq!(h(&response, header::CONTENT_RANGE), Some("bytes */1000"));
            assert!(body(response).await.is_empty());
        }
    }

    #[tokio::test]
    async fn ranges_deliver_exactly_their_bytes() {
        let (_dir, path) = fixture();
        for (header, start, end) in [
            ("bytes=100-199", 100u64, 199u64),
            ("bytes=900-", 900, 999),
            ("bytes=-50", 950, 999),
            ("bytes=0-0", 0, 0),
        ] {
            let response = range_response(&path, Some(header), false).await.unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{header}");
            assert_eq!(
                h(&response, header::CONTENT_RANGE),
                Some(format!("bytes {start}-{end}/1000").as_str())
            );
            let length = end - start + 1;
            assert_eq!(
                h(&response, header::CONTENT_LENGTH),
                Some(length.to_string().as_str())
            );
            let bytes = body(response).await;
            assert_eq!(bytes.len() as u64, length);
            assert!(
                bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == ((start as usize + i) % 256) as u8)
            );
        }
    }

    #[tokio::test]
    async fn a_large_range_streams_in_several_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.m4b");
        let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &content).unwrap();
        let response = range_response(&path, Some("bytes=1000-200999"), false)
            .await
            .unwrap();
        assert_eq!(h(&response, header::CONTENT_TYPE), Some("audio/mp4"));
        assert_eq!(body(response).await, content[1000..201_000]);
    }
}
