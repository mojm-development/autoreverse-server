//! Test harness: build the app on a test database and talk to it without a socket.

#![allow(dead_code)]

use autoreverse_server::auth::passwords::create_user;
use autoreverse_server::auth::tokens::issue_token;
use autoreverse_server::{AppState, Config, app};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, header};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub struct TestApp {
    pub state: AppState,
    router: Router,
}

impl TestApp {
    pub fn new(db: PgPool) -> Self {
        Self::with_config(db, Config::from_lookup(|_| None).unwrap())
    }

    pub fn with_config(db: PgPool, config: Config) -> Self {
        let state = AppState::new(db, config);
        TestApp {
            router: app(state.clone()),
            state,
        }
    }

    pub fn db(&self) -> &PgPool {
        &self.state.db
    }

    pub async fn send(&self, request: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub async fn get(&self, uri: &str, token: Option<&str>) -> Response<Body> {
        self.send(
            with_bearer(Request::get(uri), token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn get_with_cookie(&self, uri: &str, cookie: &str) -> Response<Body> {
        self.send(
            Request::get(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn post_json(&self, uri: &str, body: Value, token: Option<&str>) -> Response<Body> {
        self.post_raw(uri, &body.to_string(), token).await
    }

    pub async fn post_raw(&self, uri: &str, body: &str, token: Option<&str>) -> Response<Body> {
        let request = with_bearer(Request::post(uri), token)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap();
        self.send(request).await
    }

    /// Any method with an optional JSON body and bearer token.
    pub async fn call(
        &self,
        method: axum::http::Method,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> Response<Body> {
        let mut builder = with_bearer(Request::builder().method(method).uri(uri), token);
        let body = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        self.send(builder.body(body).unwrap()).await
    }

    /// A browser form POST. `cookie` is a full `Cookie` header value.
    pub async fn post_form(&self, uri: &str, form: &str, cookie: Option<&str>) -> Response<Body> {
        let mut request = Request::post(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::HOST, "localhost:3000")
            .header(header::ORIGIN, "http://localhost:3000");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::from(form.to_owned())).unwrap())
            .await
    }

    /// Creates a user and returns its id.
    pub async fn user(&self, name: &str, is_admin: bool) -> i32 {
        create_user(self.db(), name, "hunter2hunter2", is_admin)
            .await
            .unwrap()
    }

    pub async fn token(&self, user_id: i32) -> String {
        issue_token(self.db(), user_id).await.unwrap()
    }
}

fn with_bearer(
    builder: axum::http::request::Builder,
    token: Option<&str>,
) -> axum::http::request::Builder {
    match token {
        Some(token) => builder.header(header::AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    }
}

pub async fn body_text(response: Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn body_json(response: Response<Body>) -> Value {
    let text = body_text(response).await;
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}"))
}

pub fn header_str(response: &Response<Body>, name: header::HeaderName) -> &str {
    response
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap())
        .unwrap_or("")
}

/// The `name=value` part of the response's `Set-Cookie`, ready to send back as `Cookie`.
pub fn cookie_pair(response: &Response<Body>) -> String {
    header_str(response, header::SET_COOKIE)
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

/// Temporary library roots and data directory, removed when dropped.
pub struct TempLibrary {
    pub dir: tempfile::TempDir,
}

impl TempLibrary {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["books", "music", "data"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        TempLibrary { dir }
    }

    pub fn books(&self) -> std::path::PathBuf {
        self.dir.path().join("books")
    }

    pub fn music(&self) -> std::path::PathBuf {
        self.dir.path().join("music")
    }

    pub fn data(&self) -> std::path::PathBuf {
        self.dir.path().join("data")
    }

    pub fn path_str(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_owned()
    }
}

impl TestApp {
    /// The app with `AUTOREVERSE_DATA` pointing at `data` (covers land in `data/covers`).
    pub fn with_data_dir(db: PgPool, data: &std::path::Path) -> Self {
        let data = data.to_str().unwrap().to_owned();
        let config =
            Config::from_lookup(|key| (key == "AUTOREVERSE_DATA").then(|| data.clone())).unwrap();
        Self::with_config(db, config)
    }

    /// Waits until the background scan is done and returns its status.
    pub async fn wait_for_scan(&self) -> autoreverse_server::scanner::state::ScanState {
        for _ in 0..600 {
            let status = self.state.scan.snapshot();
            if !status.running {
                return status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("scan did not finish within 30 s");
    }
}

/// A one-second silent mp3 written by ffmpeg, with the given `-metadata` tags.
pub fn make_audio(path: &std::path::Path, tags: &[(&str, &str)]) {
    make_audio_with(path, tags, &[]);
}

/// Like [`make_audio`], with extra ffmpeg arguments before the output (e.g. a chapters file).
pub fn make_audio_with(path: &std::path::Path, tags: &[(&str, &str)], extra: &[&str]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut command = std::process::Command::new("ffmpeg");
    command.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=8000:cl=mono",
    ]);
    command.args(extra);
    command.args(["-t", "1"]);
    for (key, value) in tags {
        command.arg("-metadata").arg(format!("{key}={value}"));
    }
    let status = command
        .arg(path)
        .status()
        .expect("ffmpeg must be installed");
    assert!(status.success(), "ffmpeg failed for {}", path.display());
}

/// An `items` row for a test; `sort_title` is the lower-cased title, as the scanner stores it.
#[derive(Debug, Clone, Default)]
pub struct Seed<'a> {
    pub kind: &'a str,
    pub title: &'a str,
    pub author: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub series: Option<&'a str>,
    pub series_index: Option<f64>,
    pub cover_path: Option<&'a str>,
    pub source_path: Option<&'a str>,
    pub description: Option<&'a str>,
    pub narrator: Option<&'a str>,
    pub parent_id: Option<i32>,
    pub missing: bool,
    pub added_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> Seed<'a> {
    pub fn new(kind: &'a str, title: &'a str) -> Self {
        Seed {
            kind,
            title,
            ..Default::default()
        }
    }
}

pub async fn seed(db: &PgPool, item: Seed<'_>) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO items (kind, title, sort_title, author, artist, series, series_index,
             cover_path, source_path, description, narrator, parent_id, missing_since, added_at)
         VALUES ($1, $2, lower($2), $3, $4, $5, $6, $7, $8, $9, $10, $11,
             CASE WHEN $12 THEN now() END, coalesce($13, now()))
         RETURNING id",
    )
    .bind(item.kind)
    .bind(item.title)
    .bind(item.author)
    .bind(item.artist)
    .bind(item.series)
    .bind(item.series_index)
    .bind(item.cover_path)
    .bind(item.source_path)
    .bind(item.description)
    .bind(item.narrator)
    .bind(item.parent_id)
    .bind(item.missing)
    .bind(item.added_at)
    .fetch_one(db)
    .await
    .unwrap()
}

pub async fn seed_track(
    db: &PgPool,
    item_id: i32,
    position: i32,
    path: &str,
    duration: f64,
) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO tracks (item_id, position, path, duration) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(item_id)
    .bind(position)
    .bind(path)
    .bind(duration)
    .fetch_one(db)
    .await
    .unwrap()
}

pub async fn seed_progress(db: &PgPool, user_id: i32, item_id: i32, position: f64, finished: bool) {
    sqlx::query(
        "INSERT INTO progress (user_id, item_id, position, finished) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(position)
    .bind(finished)
    .execute(db)
    .await
    .unwrap();
}

type Routes = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (u16, Vec<u8>)>>>;

/// A local HTTP server standing in for the internet (feeds, artwork, episodes, the podcast
/// directory). Answers by path; unknown paths are 404. Records every request URI.
#[derive(Clone)]
pub struct FakeWeb {
    pub base: String,
    routes: Routes,
    delays: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    pub requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeWeb {
    pub async fn start() -> Self {
        let routes: Routes = Default::default();
        let requests: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let delays: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
            Default::default();
        let (r, q, d) = (routes.clone(), requests.clone(), delays.clone());
        let router = Router::new().fallback(move |uri: axum::http::Uri| {
            let (r, q, d) = (r.clone(), q.clone(), d.clone());
            async move {
                q.lock().unwrap().push(uri.to_string());
                let delay = d.lock().unwrap().get(uri.path()).copied();
                if let Some(ms) = delay {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                }
                let found = r.lock().unwrap().get(uri.path()).cloned();
                match found {
                    Some((status, body)) => {
                        (axum::http::StatusCode::from_u16(status).unwrap(), body)
                    }
                    None => (axum::http::StatusCode::NOT_FOUND, b"nope".to_vec()),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        FakeWeb {
            base: format!("http://{addr}"),
            routes,
            delays,
            requests,
        }
    }

    pub fn set(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_owned(), (status, body.into()));
    }

    /// Answers `path` only after `ms` milliseconds — for requests that must overlap.
    pub fn delay(&self, path: &str, ms: u64) {
        self.delays.lock().unwrap().insert(path.to_owned(), ms);
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn hits(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|uri| uri.split('?').next() == Some(path))
            .count()
    }
}
