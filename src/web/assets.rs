//! Versioned asset URLs. Templates link CSS and JS as `/v/<version>/css/app.css`, where the version
//! is a digest of everything in the static dir. A deploy that changes a file changes every URL, so
//! neither a browser nor a caching reverse proxy keeps serving an old file — or an error it stored
//! while the server restarted. It is a path prefix rather than a query, so the relative imports of
//! the ES modules (`./store.js`) carry it too. The unprefixed paths keep working. Only the current
//! version's files exist; the server keeps no history of earlier ones.

use std::path::Path;
use std::sync::OnceLock;

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

static VERSION: OnceLock<String> = OnceLock::new();

/// The prefix templates put before an asset path: `{{ crate::web::assets::prefix() }}/css/app.css`.
pub fn prefix() -> String {
    format!("/v/{}", VERSION.get().map_or("0", String::as_str))
}

/// Digests the static dir once per process; later calls keep the first version.
pub fn init(static_dir: &Path) {
    VERSION.get_or_init(|| digest(static_dir));
}

fn digest(static_dir: &Path) -> String {
    let mut files = Vec::new();
    collect(static_dir, static_dir, &mut files);
    files.sort();
    let mut hasher = Sha256::new();
    for (relative, contents) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(&contents);
    }
    hasher.finalize()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if let Ok(contents) = std::fs::read(&path) {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            files.push((relative.to_string_lossy().into_owned(), contents));
        }
    }
}

/// Serves `/v/<version>/css/app.css` as `/css/app.css`. Only the current version is answered with
/// the file, cacheable for a year: its URL never changes its contents. Any other version — a page
/// rendered before a deploy, or after a rollback — is redirected to the current prefix without
/// being cached, so no cache ever holds one version's file under another version's URL, and the
/// ES modules a page imports all resolve under the same prefix.
pub async fn serve_versioned(mut request: Request, next: Next) -> Response {
    let Some((version, target)) = split(request.uri()) else {
        return next.run(request).await;
    };
    let Some(current) = VERSION.get() else {
        // Not initialised (cannot happen behind `app()`): serve, but promise nothing.
        if let Ok(uri) = target.parse::<Uri>() {
            *request.uri_mut() = uri;
        }
        return next.run(request).await;
    };
    if version != current {
        return (
            StatusCode::FOUND,
            [
                (header::LOCATION, format!("/v/{current}{target}")),
                (header::CACHE_CONTROL, "no-store".to_owned()),
            ],
        )
            .into_response();
    }
    let Ok(uri) = target.parse::<Uri>() else {
        return next.run(request).await;
    };
    *request.uri_mut() = uri;
    let mut response = next.run(request).await;
    if response.status().is_success() {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}

/// `/v/abc/css/app.css?x` → (`abc`, `/css/app.css?x`).
fn split(uri: &Uri) -> Option<(&str, String)> {
    let rest = uri.path().strip_prefix("/v/")?;
    let slash = rest.find('/')?;
    let (version, path) = rest.split_at(slash);
    let target = match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    };
    Some((version, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripped(uri: &str) -> Option<(String, String)> {
        let uri: Uri = uri.parse().unwrap();
        split(&uri).map(|(version, target)| (version.to_owned(), target))
    }

    #[test]
    fn the_version_prefix_is_split_off() {
        assert_eq!(
            stripped("/v/abc123/css/app.css").unwrap(),
            ("abc123".to_owned(), "/css/app.css".to_owned())
        );
        assert_eq!(
            stripped("/v/old/js/player/store.js?x=1").unwrap(),
            ("old".to_owned(), "/js/player/store.js?x=1".to_owned())
        );
    }

    #[test]
    fn other_paths_stay() {
        assert_eq!(stripped("/css/app.css"), None);
        assert_eq!(stripped("/v/nothing-behind"), None);
        assert_eq!(stripped("/videos/a"), None);
    }

    #[test]
    fn the_digest_follows_the_contents() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        std::fs::create_dir(dir.join("css")).unwrap();
        std::fs::write(dir.join("css/app.css"), "a").unwrap();
        let first = digest(dir);
        assert_eq!(first, digest(dir));
        assert_eq!(first.len(), 12);
        std::fs::write(dir.join("css/app.css"), "b").unwrap();
        assert_ne!(first, digest(dir));
    }
}
