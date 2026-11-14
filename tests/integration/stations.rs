//! Internet radio stations: `/radio/stations…` and the page `/library/radio` (new in Rust).
//! The directory, the senders, their playlists and logos are all one local [`Sender`]; nothing
//! here reaches the internet.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::stations::SearchLogos;
use autoreverse_server::stations::live::MAX_STREAMS_PER_USER;
use autoreverse_server::stations::search_logos::Timing;
use autoreverse_server::{AppState, Config};
use image::ImageFormat;

use crate::common::{TempLibrary, TestApp, body_json, body_text, header_str};

/// A logo small enough to be served as it is at every size.
static PNG: LazyLock<Vec<u8>> = LazyLock::new(|| picture(8, ImageFormat::Png));
const UUID: &str = "d28420a4-eccf-47a2-ace1-088c7e7cb7e0";
/// The homepage of the stations that are not about homepages: an address of the documentation
/// range, which the fence refuses without a packet — no test may wait for a name to resolve.
const HOMEPAGE: &str = "https://192.0.2.1/";
const HLS_TEXT: &str = "HLS-Sender (.m3u8) werden noch nicht unterstützt. Bitte die MP3- oder AAC-Adresse des Senders verwenden.";
const NO_AUDIO_TEXT: &str = "Unter dieser Adresse gibt es keinen Audio-Stream. Bitte die direkte Stream-Adresse des Senders angeben.";

/// A square picture with a transparent corner, `edge` pixels wide.
fn picture(edge: u32, format: ImageFormat) -> Vec<u8> {
    let mut image = image::RgbaImage::from_pixel(edge, edge, image::Rgba([200, 40, 40, 255]));
    image.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
    let mut data = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut data), format)
        .unwrap();
    data
}

/// What the local server answers under a path.
#[derive(Clone)]
enum Reply {
    Fixed {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    },
    /// A PNG that takes its time.
    Slow(Duration),
    /// A redirect to a path here.
    Moved(&'static str),
    /// An endless stream of `a` bytes. Asked with `Icy-MetaData: 1` it weaves a metadata block
    /// in after every `metaint` bytes: the title first, "no change" from then on.
    Live {
        content_type: &'static str,
        metaint: Option<usize>,
        title: &'static str,
    },
}

#[derive(Default)]
struct SenderState {
    routes: Mutex<HashMap<String, Reply>>,
    requests: Mutex<Vec<(String, HeaderMap)>>,
    /// Live streams somebody is connected to right now.
    open: AtomicUsize,
}

/// Counts a live stream for as long as its body exists.
struct Open(Arc<SenderState>);

impl Drop for Open {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A local HTTP server standing in for the directory and for every sender.
#[derive(Clone)]
struct Sender {
    base: String,
    state: Arc<SenderState>,
}

fn metadata_block(title: &str) -> Vec<u8> {
    let text = format!("StreamTitle='{title}';StreamUrl='';");
    let blocks = text.len().div_ceil(16);
    let mut out = vec![blocks as u8];
    out.extend_from_slice(text.as_bytes());
    out.resize(1 + blocks * 16, 0);
    out
}

fn live_body(state: Arc<SenderState>, metaint: Option<usize>, title: &'static str) -> Body {
    state.open.fetch_add(1, Ordering::SeqCst);
    let guard = Open(state);
    let stream = futures_util::stream::unfold((guard, 0u32), move |(guard, turn)| async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let chunk = match metaint {
            None => vec![b'a'; 512],
            Some(interval) => {
                let mut chunk = vec![b'a'; interval];
                if turn == 0 {
                    chunk.extend(metadata_block(title));
                } else {
                    chunk.push(0);
                }
                chunk
            }
        };
        Some((
            Ok::<_, std::io::Error>(Bytes::from(chunk)),
            (guard, turn + 1),
        ))
    });
    Body::from_stream(stream)
}

impl Sender {
    async fn start() -> Sender {
        let state: Arc<SenderState> = Arc::default();
        let shared = state.clone();
        let router = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
            let state = shared.clone();
            async move {
                state
                    .requests
                    .lock()
                    .unwrap()
                    .push((uri.to_string(), headers.clone()));
                let reply = state.routes.lock().unwrap().get(uri.path()).cloned();
                match reply {
                    None => (StatusCode::NOT_FOUND, "nope").into_response(),
                    Some(Reply::Fixed {
                        status,
                        content_type,
                        body,
                    }) => {
                        let status = StatusCode::from_u16(status).unwrap();
                        if content_type.is_empty() {
                            (status, body).into_response()
                        } else {
                            (status, [(header::CONTENT_TYPE, content_type)], body).into_response()
                        }
                    }
                    Some(Reply::Moved(to)) => {
                        (StatusCode::FOUND, [(header::LOCATION, to)]).into_response()
                    }
                    Some(Reply::Slow(delay)) => {
                        tokio::time::sleep(delay).await;
                        ([(header::CONTENT_TYPE, "image/png")], PNG.clone()).into_response()
                    }
                    Some(Reply::Live {
                        content_type,
                        metaint,
                        title,
                    }) => {
                        let wanted = headers.get("icy-metadata").is_some_and(|v| v == "1");
                        let metaint = metaint.filter(|_| wanted);
                        let mut response = Response::new(live_body(state, metaint, title));
                        let out = response.headers_mut();
                        out.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
                        out.insert("icy-br", "128".parse().unwrap());
                        if let Some(interval) = metaint {
                            out.insert("icy-metaint", interval.to_string().parse().unwrap());
                        }
                        response
                    }
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Sender {
            base: format!("http://{addr}"),
            state,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn fixed(&self, path: &str, status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) {
        self.state.routes.lock().unwrap().insert(
            path.to_owned(),
            Reply::Fixed {
                status,
                content_type,
                body: body.into(),
            },
        );
    }

    /// An MP3 sender with ICY titles every 64 bytes.
    fn live(&self, path: &str, title: &'static str) {
        self.live_as(path, "audio/mpeg", Some(64), title);
    }

    fn live_as(
        &self,
        path: &str,
        content_type: &'static str,
        metaint: Option<usize>,
        title: &'static str,
    ) {
        self.state.routes.lock().unwrap().insert(
            path.to_owned(),
            Reply::Live {
                content_type,
                metaint,
                title,
            },
        );
    }

    fn slow(&self, path: &str, delay: Duration) {
        let mut routes = self.state.routes.lock().unwrap();
        routes.insert(path.to_owned(), Reply::Slow(delay));
    }

    fn moved(&self, path: &str, to: &'static str) {
        let mut routes = self.state.routes.lock().unwrap();
        routes.insert(path.to_owned(), Reply::Moved(to));
    }

    fn json(&self, path: &str, body: Value) {
        self.fixed(path, 200, "application/json", body.to_string());
    }

    fn hits(&self, path: &str) -> usize {
        let requests = self.state.requests.lock().unwrap();
        requests
            .iter()
            .filter(|(uri, _)| uri.split('?').next() == Some(path))
            .count()
    }

    fn last_request(&self, path: &str) -> (String, HeaderMap) {
        let requests = self.state.requests.lock().unwrap();
        requests
            .iter()
            .rev()
            .find(|(uri, _)| uri.split('?').next() == Some(path))
            .cloned()
            .unwrap_or_else(|| panic!("no request for {path}"))
    }

    fn open_streams(&self) -> usize {
        self.state.open.load(Ordering::SeqCst)
    }

    /// A directory entry whose stream and logo live on this server.
    fn entry(&self, id: &str, name: &str, stream: &str) -> Value {
        json!({
            "stationuuid": id, "name": name,
            "url": self.url(stream), "url_resolved": self.url(stream),
            "homepage": HOMEPAGE, "favicon": self.url("/logo.png"),
            "tags": "jazz,smooth jazz", "countrycode": "DE", "codec": "MP3", "bitrate": 192,
            "hls": 0, "lastcheckok": 1,
        })
    }

    /// A directory entry with the logo under `logo` (a path here, or an address anywhere) and
    /// no homepage, so that the directory's logo is all there is.
    fn entry_with_logo(&self, id: &str, logo: &str) -> Value {
        self.entry_with_page(id, logo, "")
    }

    /// A directory entry with logo and homepage under `logo` and `page` (paths here, or
    /// addresses anywhere; empty for none).
    fn entry_with_page(&self, id: &str, logo: &str, page: &str) -> Value {
        let address = |raw: &str| {
            if raw.starts_with('/') {
                json!(self.url(raw))
            } else {
                json!(raw)
            }
        };
        let mut entry = self.entry(id, "Sender", "/s");
        entry["favicon"] = address(logo);
        entry["homepage"] = address(page);
        entry
    }

    /// A web page under `path` with `head` in its head.
    fn page(&self, path: &str, head: &str) {
        let html = format!(
            "<!doctype html><html><head><title>Sender</title>{head}</head><body></body></html>"
        );
        self.fixed(path, 200, "text/html; charset=utf-8", html);
    }

    /// The directory knows `entry` under its id.
    fn knows(&self, entry: &Value) {
        let id = entry["stationuuid"].as_str().unwrap();
        self.json(&format!("/json/stations/byuuid/{id}"), json!([entry]));
    }
}

/// The configuration with the data in `data`, the directory on `sender`, and whatever else
/// `env` sets.
fn config_with(data: &Path, sender: &Sender, env: &[(&str, &str)]) -> Config {
    let mut vars: HashMap<String, String> = env
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    vars.insert("AUTOREVERSE_DATA".into(), data.to_str().unwrap().into());
    vars.insert(
        "AUTOREVERSE_RADIO_DIRECTORY_URL".into(),
        sender.base.clone(),
    );
    Config::from_lookup(|key| vars.get(key).cloned()).unwrap()
}

struct World {
    _library: TempLibrary,
    sender: Sender,
    app: TestApp,
    token: String,
    user: i32,
}

async fn world_with(db: PgPool, env: &[(&str, &str)]) -> World {
    world_timed(db, env, Timing::default()).await
}

/// A world whose logos of search hits wait and are kept as `timing` says.
async fn world_timed(db: PgPool, env: &[(&str, &str)], timing: Timing) -> World {
    let library = TempLibrary::new();
    let sender = Sender::start().await;
    let mut state = AppState::new(db, config_with(&library.data(), &sender, env));
    let settings = autoreverse_server::settings::from_env(|key| {
        env.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_owned())
    });
    state.settings.set(settings);
    state.radio_logos = Arc::new(SearchLogos::new(timing));
    let app = TestApp::with_state(state);
    let user = app.user("ada", false).await;
    let token = app.token(user).await;
    World {
        _library: library,
        sender,
        app,
        token,
        user,
    }
}

async fn world(db: PgPool) -> World {
    world_with(db, &[]).await
}

impl World {
    async fn post(&self, body: Value) -> Response<Body> {
        self.app
            .post_json("/radio/stations", body, Some(&self.token))
            .await
    }

    /// Adds a live MP3 sender under `path` by its address and returns the station's id.
    async fn station(&self, path: &str, name: &str) -> i64 {
        self.sender.live(path, "Kraftwerk - Autobahn");
        let response = self
            .post(json!({"stream_url": self.sender.url(path), "name": name}))
            .await;
        assert_eq!(response.status(), StatusCode::CREATED, "{path}");
        body_json(response).await["id"].as_i64().unwrap()
    }

    async fn names(&self) -> Vec<String> {
        let body = body_json(self.app.get("/radio/stations", Some(&self.token)).await).await;
        body["stations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|station| station["name"].as_str().unwrap().to_owned())
            .collect()
    }

    async fn positions(&self) -> Vec<i32> {
        sqlx::query_scalar(
            "SELECT position FROM radio_stations WHERE user_id = $1 ORDER BY position",
        )
        .bind(self.user)
        .fetch_all(self.app.db())
        .await
        .unwrap()
    }

    async fn now(&self, id: i64) -> Value {
        let uri = format!("/radio/stations/{id}/now");
        body_json(self.app.get(&uri, Some(&self.token)).await).await
    }
}

async fn detail(response: Response<Body>) -> String {
    body_json(response).await["detail"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Reads at least `count` bytes off a streaming body.
async fn read_bytes(body: &mut Body, count: usize) -> Vec<u8> {
    let mut got = Vec::new();
    while got.len() < count {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("the stream stalled")
            .expect("the stream ended")
            .expect("the stream broke");
        if let Ok(data) = frame.into_data() {
            got.extend_from_slice(&data);
        }
    }
    got
}

/// Waits (up to five seconds) until `done` holds.
async fn eventually(what: &str, mut done: impl AsyncFnMut() -> bool) {
    for _ in 0..500 {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

// --- The list -----------------------------------------------------------------------------

#[sqlx::test]
async fn the_list_is_empty_at_first_and_needs_a_login(db: PgPool) {
    let w = world(db).await;
    let response = w.app.get("/radio/stations", Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!({"stations": []}));

    for uri in [
        "/radio/stations",
        "/radio/stations/search?q=jazz",
        "/radio/stations/search/logo?id=d28420a4-eccf-47a2-ace1-088c7e7cb7e0",
        "/radio/stations/1/stream",
        "/radio/stations/1/now",
        "/radio/stations/1/logo",
    ] {
        let response = w.app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    let response = w.app.post_json("/radio/stations", json!({}), None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// --- The directory ------------------------------------------------------------------------

#[sqlx::test]
async fn the_search_maps_the_directory_and_marks_what_the_user_has(db: PgPool) {
    let w = world(db).await;
    let mine = w.sender.entry(UUID, "Jazz Eins", "/eins");
    w.sender.live("/eins", "x");
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender.knows(&mine);
    assert_eq!(
        w.post(json!({"directory_id": UUID})).await.status(),
        StatusCode::CREATED
    );

    let mut hls = w.sender.entry(
        "aaaaaaaa-0000-0000-0000-000000000001",
        "Jazz HLS",
        "/hls.m3u8",
    );
    hls["hls"] = json!(1);
    let mut offline = w
        .sender
        .entry("aaaaaaaa-0000-0000-0000-000000000002", "Jazz Aus", "/aus");
    offline["lastcheckok"] = json!(0);
    let mut bare = w
        .sender
        .entry("aaaaaaaa-0000-0000-0000-000000000003", "Jazz Zwei", "/zwei");
    for key in ["homepage", "favicon", "tags", "countrycode"] {
        bare[key] = json!("");
    }
    bare["codec"] = json!("UNKNOWN");
    bare["bitrate"] = json!(0);
    w.sender
        .json("/json/stations/search", json!([mine, hls, offline, bare]));

    let uri = "/radio/stations/search?q=%20jazz%20&country=de&limit=10";
    let response = w.app.get(uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({"results": [
            {"directory_id": UUID, "name": "Jazz Eins", "homepage": HOMEPAGE,
             "country": "DE", "tags": ["jazz", "smooth jazz"], "codec": "MP3", "bitrate": 192,
             "logo_url": w.sender.url("/logo.png"), "has_logo": true, "added": true},
            {"directory_id": "aaaaaaaa-0000-0000-0000-000000000003", "name": "Jazz Zwei",
             "homepage": null, "country": null, "tags": [], "codec": null, "bitrate": null,
             "logo_url": null, "has_logo": false, "added": false},
        ]})
    );
    // What was asked: the words, the country, only working stations, and room for filtering.
    let (asked, headers) = w.sender.last_request("/json/stations/search");
    let query: HashMap<String, String> =
        serde_urlencoded::from_str(asked.split_once('?').unwrap().1).unwrap();
    assert_eq!(query["name"], "jazz");
    assert_eq!(query["countrycode"], "DE");
    assert_eq!(query["hidebroken"], "true");
    assert_eq!(query["limit"], "20");
    assert!(
        headers[header::USER_AGENT]
            .to_str()
            .unwrap()
            .starts_with("Autoreverse/")
    );

    // Somebody else has none of them.
    let other = w.app.user("bob", false).await;
    let other = w.app.token(other).await;
    let body = body_json(w.app.get(uri, Some(&other)).await).await;
    assert_eq!(body["results"][0]["added"], false);
}

#[sqlx::test]
async fn the_search_cuts_the_list_at_the_limit_and_checks_its_parameters(db: PgPool) {
    let w = world(db).await;
    let entries: Vec<Value> = (0..5)
        .map(|i| {
            w.sender.entry(
                &format!("aaaaaaaa-0000-0000-0000-00000000000{i}"),
                "Jazz",
                "/s",
            )
        })
        .collect();
    w.sender.json("/json/stations/search", json!(entries));
    let body = body_json(
        w.app
            .get("/radio/stations/search?q=jazz&limit=2", Some(&w.token))
            .await,
    )
    .await;
    assert_eq!(body["results"].as_array().unwrap().len(), 2);
    // No country named: none asked for.
    assert!(
        !w.sender
            .last_request("/json/stations/search")
            .0
            .contains("countrycode")
    );

    for (uri, text) in [
        ("/radio/stations/search", "q muss 1–100 Zeichen haben"),
        ("/radio/stations/search?q=%20", "q muss 1–100 Zeichen haben"),
        (
            "/radio/stations/search?q=jazz&limit=0",
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/radio/stations/search?q=jazz&limit=51",
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/radio/stations/search?q=jazz&country=deu",
            "country muss ein Ländercode aus 2 Buchstaben sein",
        ),
    ] {
        let response = w.app.get(uri, Some(&w.token)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert_eq!(detail(response).await, text, "{uri}");
    }
}

#[sqlx::test]
async fn an_unreachable_or_confused_directory_is_a_502(db: PgPool) {
    let w = world(db).await;
    let uri = "/radio/stations/search?q=jazz";
    // Nothing configured on the stub: 404 from the directory.
    let response = w.app.get(uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(detail(response).await, "Senderverzeichnis nicht erreichbar");

    w.sender.fixed(
        "/json/stations/search",
        200,
        "text/html",
        "<html>Wartung</html>",
    );
    let response = w.app.get(uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        detail(response).await,
        "Unerwartete Antwort des Senderverzeichnisses"
    );

    // Adding from it fails the same way.
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

// --- Adding -------------------------------------------------------------------------------

#[sqlx::test]
async fn a_station_from_the_directory_brings_its_data_and_logo(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender.knows(&w.sender.entry(UUID, "Jazz Eins", "/jazz"));

    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    let id = station["id"].as_i64().unwrap();
    assert_eq!(
        station,
        json!({"id": id, "name": "Jazz Eins", "homepage": HOMEPAGE,
               "country": "DE", "tags": ["jazz", "smooth jazz"], "codec": "MP3", "bitrate": 192,
               "has_logo": true})
    );
    let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
    assert_eq!(list, json!({"stations": [station]}));

    // The logo comes from the server's own disk.
    let uri = format!("/radio/stations/{id}/logo");
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        PNG.as_slice()
    );
    let sized = w.app.get(&format!("{uri}?size=128"), Some(&w.token)).await;
    assert_eq!(sized.status(), StatusCode::OK);
    let broken = w.app.get(&format!("{uri}?size=x"), Some(&w.token)).await;
    assert_eq!(broken.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(w.sender.hits("/logo.png"), 1);

    // A second time, in whatever spelling: the same station, and nobody is asked again.
    let again = w.post(json!({"directory_id": UUID.to_uppercase()})).await;
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(body_json(again).await, station);
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 1);
    assert_eq!(w.names().await, ["Jazz Eins"]);

    // Its address typed in by hand is the same station too.
    let by_hand = w
        .post(json!({"stream_url": w.sender.url("/jazz"), "name": "Anders"}))
        .await;
    assert_eq!(by_hand.status(), StatusCode::OK);
    assert_eq!(body_json(by_hand).await["id"], id);
}

#[sqlx::test]
async fn a_large_transparent_logo_stays_transparent_when_scaled(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    // A red disc on a clear ground, as station logos come.
    let mut logo = image::RgbaImage::new(600, 600);
    for (x, y, pixel) in logo.enumerate_pixels_mut() {
        let (dx, dy) = (x as i64 - 300, y as i64 - 300);
        if dx * dx + dy * dy < 200 * 200 {
            *pixel = image::Rgba([200, 40, 40, 255]);
        }
    }
    let mut large = Vec::new();
    logo.write_to(&mut std::io::Cursor::new(&mut large), ImageFormat::Png)
        .unwrap();
    w.sender
        .fixed("/logo.png", 200, "image/png", large.as_slice());
    w.sender.knows(&w.sender.entry(UUID, "Jazz Eins", "/jazz"));
    let station = body_json(w.post(json!({"directory_id": UUID})).await).await;
    let uri = format!("/radio/stations/{}/logo?size=128", station["id"]);

    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    let data = response.into_body().collect().await.unwrap().to_bytes();
    let thumb = image::load_from_memory(&data).unwrap().into_rgba8();
    assert_eq!((thumb.width(), thumb.height()), (128, 128));
    assert_eq!(thumb.get_pixel(0, 0)[3], 0, "no black corner");
    assert_eq!(thumb.get_pixel(64, 64)[3], 255);
    // From the cache the second time, still a PNG.
    let again = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(header_str(&again, header::CONTENT_TYPE), "image/png");
}

#[sqlx::test]
async fn what_the_directory_cannot_deliver_is_not_added(db: PgPool) {
    let w = world(db).await;
    // An id it does not know (it answers with an empty list), and one that is none.
    let unknown = "aaaaaaaa-0000-0000-0000-00000000dead";
    w.sender
        .json(&format!("/json/stations/byuuid/{unknown}"), json!([]));
    for id in [unknown, "../stats"] {
        let response = w.post(json!({"directory_id": id})).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
        assert_eq!(
            detail(response).await,
            "Diesen Sender kennt das Verzeichnis nicht (mehr)"
        );
    }
    // An HLS station.
    let mut hls = w.sender.entry(UUID, "HLS", "/hls.m3u8");
    hls["hls"] = json!(1);
    w.sender.knows(&hls);
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, HLS_TEXT);
    // A station whose sender is gone, whatever the directory's last check said.
    let gone = "aaaaaaaa-0000-0000-0000-000000000009";
    w.sender.knows(&w.sender.entry(gone, "Weg", "/weg"));
    let response = w.post(json!({"directory_id": gone})).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        detail(response).await,
        "Der Sender ist unter dieser Adresse nicht erreichbar."
    );
    assert!(w.names().await.is_empty());
}

#[sqlx::test]
async fn a_logo_that_is_no_image_leaves_the_station_without_one(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    w.sender
        .fixed("/logo.png", 200, "image/png", "<svg onload=alert(1)>");
    w.sender.knows(&w.sender.entry(UUID, "Jazz Eins", "/jazz"));
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(station["has_logo"], false);
    let uri = format!("/radio/stations/{}/logo", station["id"]);
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Kein Logo");
}

#[sqlx::test]
async fn a_station_by_its_address_is_checked_and_described_by_its_sender(db: PgPool) {
    let w = world(db).await;
    w.sender.live_as("/aac", "audio/aacp; charset=x", None, "x");
    let response = w
        .post(
            json!({"stream_url": format!(" {} ", w.sender.url("/aac")), "name": "  Mein Sender ",
                     "homepage": HOMEPAGE}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(
        station,
        json!({"id": station["id"], "name": "Mein Sender", "homepage": HOMEPAGE,
               "country": null, "tags": [], "codec": "AAC", "bitrate": 128, "has_logo": false})
    );
    // The check does not ask for metadata: it wants to see audio.
    assert!(
        w.sender
            .last_request("/aac")
            .1
            .get("icy-metadata")
            .is_none()
    );
    let stored: (String, Option<String>) =
        sqlx::query_as("SELECT stream_url, content_type FROM radio_stations WHERE id = $1")
            .bind(station["id"].as_i64().unwrap() as i32)
            .fetch_one(w.app.db())
            .await
            .unwrap();
    assert_eq!(
        stored,
        (w.sender.url("/aac"), Some("audio/aacp".to_owned()))
    );

    // The same address again is the same station; the sender is not asked a second time.
    let again = w
        .post(json!({"stream_url": w.sender.url("/aac"), "name": "Nochmal"}))
        .await;
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(body_json(again).await, station);
    assert_eq!(w.sender.hits("/aac"), 1);
}

#[sqlx::test]
async fn playlist_files_are_followed_to_the_stream(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/live/one", "x");
    w.sender.live("/live/two", "x");
    w.sender.live("/live/three", "x");
    // An m3u naming an absolute address, under the type senders use for it.
    let m3u = format!(
        "#EXTM3U\r\n#EXTINF:-1,Eins\r\n{}\r\n",
        w.sender.url("/live/one")
    );
    w.sender.fixed("/listen.m3u", 200, "audio/x-mpegurl", m3u);
    // A pls naming a relative address, served as plain text.
    let pls = "[playlist]\nNumberOfEntries=1\nFile1=two\nTitle1=Zwei\n";
    w.sender.fixed("/live/listen.pls", 200, "text/plain", pls);
    // An m3u that names a pls that names the stream, without any media type.
    w.sender
        .fixed("/outer.m3u", 200, "", w.sender.url("/inner.pls"));
    let inner = format!("[playlist]\nFile1={}\n", w.sender.url("/live/three"));
    w.sender.fixed("/inner.pls", 200, "audio/x-scpls", inner);

    for (entered, stream, name) in [
        ("/listen.m3u", "/live/one", "Eins"),
        ("/live/listen.pls", "/live/two", "Zwei"),
        ("/outer.m3u", "/live/three", "Drei"),
    ] {
        let response = w
            .post(json!({"stream_url": w.sender.url(entered), "name": name}))
            .await;
        assert_eq!(response.status(), StatusCode::CREATED, "{entered}");
        let id = body_json(response).await["id"].as_i64().unwrap() as i32;
        let stored: (String, String) =
            sqlx::query_as("SELECT source_url, stream_url FROM radio_stations WHERE id = $1")
                .bind(id)
                .fetch_one(w.app.db())
                .await
                .unwrap();
        assert_eq!(
            stored,
            (w.sender.url(entered), w.sender.url(stream)),
            "{entered}"
        );
        // Entered once more, as the playlist or as the stream behind it: the same station.
        for address in [entered, stream] {
            let again = w
                .post(json!({"stream_url": w.sender.url(address), "name": "Doppelt"}))
                .await;
            assert_eq!(again.status(), StatusCode::OK, "{address}");
            assert_eq!(body_json(again).await["id"], id, "{address}");
        }
    }
    assert_eq!(w.names().await, ["Eins", "Zwei", "Drei"]);
}

#[sqlx::test]
async fn an_address_that_is_no_stream_is_refused_with_a_reason(db: PgPool) {
    let w = world(db).await;
    w.sender.fixed(
        "/page",
        200,
        "text/html; charset=utf-8",
        "<!doctype html><html>",
    );
    w.sender.fixed("/gone", 404, "text/plain", "nope");
    w.sender.fixed("/silent", 200, "audio/mpeg", "");
    w.sender
        .fixed("/empty.m3u", 200, "audio/x-mpegurl", "#EXTM3U\n# nichts\n");
    w.sender
        .fixed("/odd.m3u", 200, "audio/x-mpegurl", "rtsp://x/stream\n");
    w.sender.fixed(
        "/loop.m3u",
        200,
        "audio/x-mpegurl",
        w.sender.url("/loop.m3u"),
    );
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg-1.aac\n";
    w.sender.fixed(
        "/hls/live.m3u8",
        200,
        "application/vnd.apple.mpegurl",
        media,
    );
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=96000\nlow/index.m3u8\n";
    w.sender
        .fixed("/hls/master.m3u8", 200, "audio/mpegurl", master);
    // To the fenced client the app's own loopback port is as unreachable as the router.
    let unreachable = "Der Sender ist unter dieser Adresse nicht erreichbar.";
    let cases = [
        (w.sender.url("/page"), NO_AUDIO_TEXT),
        (w.sender.url("/gone"), unreachable),
        (w.sender.url("/nowhere"), unreachable),
        (
            w.sender.url("/silent"),
            "Der Sender antwortet, liefert aber keinen Ton.",
        ),
        (
            w.sender.url("/empty.m3u"),
            "Die Playlist-Datei nennt keine Stream-Adresse.",
        ),
        (
            w.sender.url("/odd.m3u"),
            "Die Playlist-Datei nennt keine Stream-Adresse.",
        ),
        (
            w.sender.url("/loop.m3u"),
            "Die Adresse verweist zu oft auf weitere Playlist-Dateien.",
        ),
        (w.sender.url("/hls/live.m3u8"), HLS_TEXT),
        (w.sender.url("/hls/master.m3u8"), HLS_TEXT),
        (
            "ftp://sender.example/live".to_owned(),
            "Die Adresse muss mit http:// oder https:// beginnen.",
        ),
        (
            "sender.example/live".to_owned(),
            "Die Adresse muss mit http:// oder https:// beginnen.",
        ),
        (
            "http://127.0.0.1:9/live".to_owned(),
            "Adressen im lokalen Netz können nicht als Sender hinzugefügt werden.",
        ),
        (
            "http://192.168.1.1/".to_owned(),
            "Adressen im lokalen Netz können nicht als Sender hinzugefügt werden.",
        ),
        (
            "http://[::1]:8000/live".to_owned(),
            "Adressen im lokalen Netz können nicht als Sender hinzugefügt werden.",
        ),
        // A name that resolves into the local network gets no further than the resolver.
        ("http://localhost:9/live".to_owned(), unreachable),
    ];
    for (address, text) in cases {
        let response = w
            .post(json!({"stream_url": address, "name": "Sender"}))
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{address}"
        );
        assert_eq!(detail(response).await, text, "{address}");
    }
    assert!(w.names().await.is_empty());
}

#[sqlx::test]
async fn a_playlist_cannot_lead_the_server_into_the_local_network(db: PgPool) {
    let w = world(db).await;
    // The sender is the configured directory host, so it may be local itself — but what its
    // playlist names is somebody else's word again.
    w.sender.fixed(
        "/evil.m3u",
        200,
        "audio/x-mpegurl",
        "http://127.0.0.1:9/admin\n",
    );
    let response = w
        .post(json!({"stream_url": w.sender.url("/evil.m3u"), "name": "Böse"}))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        detail(response).await,
        "Adressen im lokalen Netz können nicht als Sender hinzugefügt werden."
    );
}

#[sqlx::test]
async fn a_body_must_name_one_way_to_add(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    let url = w.sender.url("/jazz");
    let one_way = "Entweder directory_id oder stream_url und name müssen angegeben sein";
    let name = "Der Name muss 1–100 Zeichen lang sein.";
    for (body, text) in [
        (json!({}), one_way),
        (json!({"stream_url": url}), one_way),
        (json!({"name": "Sender"}), one_way),
        (
            json!({"directory_id": UUID, "stream_url": url, "name": "Sender"}),
            one_way,
        ),
        (
            json!({"directory_id": 7}),
            "directory_id muss eine Zeichenkette sein",
        ),
        (
            json!({"stream_url": url, "name": 7}),
            "name muss eine Zeichenkette sein",
        ),
        (json!({"stream_url": url, "name": "   "}), name),
        (json!({"stream_url": url, "name": "x".repeat(101)}), name),
        (
            json!({"stream_url": url, "name": "Sender", "homepage": "sender.example"}),
            "Die Webseite muss mit http:// oder https:// beginnen.",
        ),
    ] {
        let response = w.post(body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(detail(response).await, text, "{body}");
    }
    let response = w
        .app
        .post_raw("/radio/stations", "{nope", Some(&w.token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");
    assert_eq!(w.sender.hits("/jazz"), 0);
}

// --- Renaming, ordering, removing ---------------------------------------------------------

#[sqlx::test]
async fn stations_are_renamed_and_moved_with_gapless_positions(db: PgPool) {
    let w = world(db).await;
    let a = w.station("/a", "A").await;
    let b = w.station("/b", "B").await;
    let c = w.station("/c", "C").await;
    assert_eq!(w.names().await, ["A", "B", "C"]);
    let patch = |id: i64, body: Value| {
        let uri = format!("/radio/stations/{id}");
        let (app, token) = (&w.app, &w.token);
        async move { app.call(Method::PATCH, &uri, Some(body), Some(token)).await }
    };

    let response = patch(c, json!({"position": 1})).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["id"], c);
    assert_eq!(w.names().await, ["C", "A", "B"]);
    // Later in the list, and past its end.
    patch(c, json!({"position": 2})).await;
    assert_eq!(w.names().await, ["A", "C", "B"]);
    patch(a, json!({"position": 99})).await;
    assert_eq!(w.names().await, ["C", "B", "A"]);
    assert_eq!(w.positions().await, [1, 2, 3]);

    // Both at once.
    let response = patch(b, json!({"name": " Bee ", "position": 1})).await;
    let body = body_json(response).await;
    assert_eq!(body["name"], "Bee");
    assert_eq!(w.names().await, ["Bee", "C", "A"]);

    let one_of = "name (1–100 Zeichen) oder position (ganze Zahl ab 1) muss angegeben sein";
    let name = "Der Name muss 1–100 Zeichen lang sein.";
    for (body, text) in [
        (json!({}), one_of),
        (json!({"position": 0}), one_of),
        (json!({"position": 1.5}), one_of),
        (json!({"position": "1"}), one_of),
        (json!({"name": ""}), name),
        (json!({"name": 5}), name),
    ] {
        let response = patch(a, body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(detail(response).await, text, "{body}");
    }
    assert_eq!(w.names().await, ["Bee", "C", "A"]);

    // Removing closes the gap.
    let uri = format!("/radio/stations/{c}");
    let response = w.app.call(Method::DELETE, &uri, None, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(w.names().await, ["Bee", "A"]);
    assert_eq!(w.positions().await, [1, 2]);
    let again = w.app.call(Method::DELETE, &uri, None, Some(&w.token)).await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn removing_a_station_removes_its_logo_file(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender.knows(&w.sender.entry(UUID, "Jazz Eins", "/jazz"));
    let id = body_json(w.post(json!({"directory_id": UUID})).await).await["id"]
        .as_i64()
        .unwrap();
    let file = w.app.state.config.radio_dir.join(format!("logos/{id}.png"));
    assert!(file.is_file());
    let uri = format!("/radio/stations/{id}");
    w.app.call(Method::DELETE, &uri, None, Some(&w.token)).await;
    assert!(!file.exists());
}

#[sqlx::test]
async fn another_users_station_does_not_exist(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/a", "A").await;
    let other = w.app.user("bob", false).await;
    let other = w.app.token(other).await;
    assert_eq!(
        body_json(w.app.get("/radio/stations", Some(&other)).await).await,
        json!({"stations": []})
    );
    let hits = w.sender.hits("/a");
    // Somebody else's, one that never was, and something that is no id: all the same answer.
    for (id, token) in [
        (id.to_string(), &other),
        ("999999".to_owned(), &w.token),
        ("abc".to_owned(), &w.token),
    ] {
        for suffix in ["/stream", "/now", "/logo"] {
            let uri = format!("/radio/stations/{id}{suffix}");
            let response = w.app.get(&uri, Some(token)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(detail(response).await, "Unbekannter Sender", "{uri}");
        }
        let uri = format!("/radio/stations/{id}");
        let patch = json!({"name": "Meins"});
        let response = w
            .app
            .call(Method::PATCH, &uri, Some(patch), Some(token))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        let response = w.app.call(Method::DELETE, &uri, None, Some(token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    // Nothing of it reached the sender or the list.
    assert_eq!(w.sender.hits("/a"), hits);
    assert_eq!(w.names().await, ["A"]);
    // Each user has a list of their own, positions and all: the same sender twice is fine.
    let response = w
        .app
        .post_json(
            "/radio/stations",
            json!({"stream_url": w.sender.url("/a"), "name": "Bobs"}),
            Some(&other),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

// --- Listening ----------------------------------------------------------------------------

#[sqlx::test]
async fn the_stream_is_pure_audio_and_its_title_is_remembered(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let now_idle = w.now(id).await;
    assert_eq!(
        now_idle,
        json!({"title": null, "artist": null, "raw": null, "listening": false})
    );

    let uri = format!("/radio/stations/{id}/stream");
    let request = Request::get(&uri)
        .header(header::AUTHORIZATION, format!("Bearer {}", w.token))
        // A live stream has no positions; a range is not an error, it is just not honoured.
        .header(header::RANGE, "bytes=100-")
        .body(Body::empty())
        .unwrap();
    let response = w.app.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/mpeg");
    assert_eq!(header_str(&response, header::CACHE_CONTROL), "no-store");
    assert_eq!(header_str(&response, header::ACCEPT_RANGES), "none");
    assert!(response.headers().get(header::CONTENT_LENGTH).is_none());
    assert!(response.headers().get(header::CONTENT_RANGE).is_none());
    assert!(response.headers().get("icy-metaint").is_none());
    // The sender was asked for its metadata …
    assert_eq!(w.sender.last_request("/live").1["icy-metadata"], "1");

    // … and none of it reaches the client: several intervals of nothing but audio.
    let mut body = response.into_body();
    let audio = read_bytes(&mut body, 64 * 6).await;
    assert!(
        audio.iter().all(|&byte| byte == b'a'),
        "metadata leaked into the audio"
    );

    assert_eq!(
        w.now(id).await,
        json!({"title": "Autobahn", "artist": "Kraftwerk", "raw": "Kraftwerk - Autobahn",
               "listening": true})
    );
    assert_eq!(w.sender.open_streams(), 1);

    // The client goes away: so does the connection to the sender, and the title with it.
    drop(body);
    eventually("the sender's connection closes", async || {
        w.sender.open_streams() == 0
    })
    .await;
    assert_eq!(w.now(id).await, now_idle);
}

/// The same over a real connection, as a player has it: the server only learns that the client
/// is gone from the socket.
#[sqlx::test]
async fn a_client_that_hangs_up_ends_the_connection_to_the_sender(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = autoreverse_server::app(w.app.state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = reqwest::Client::new();
    let mut response = client
        .get(format!("http://{address}/radio/stations/{id}/stream"))
        .bearer_auth(&w.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "audio/mpeg");
    // No length: the body is sent as it comes.
    assert!(response.headers().get("content-length").is_none());
    assert_eq!(response.headers()["transfer-encoding"], "chunked");
    let mut heard = 0;
    while heard < 64 * 4 {
        let chunk = response.chunk().await.unwrap().expect("the stream ended");
        assert!(chunk.iter().all(|&byte| byte == b'a'));
        heard += chunk.len();
    }
    assert_eq!(w.sender.open_streams(), 1);
    assert_eq!(w.now(id).await["listening"], true);

    drop(response);
    drop(client);
    eventually("the sender's connection closes", async || {
        w.sender.open_streams() == 0
    })
    .await;
    assert_eq!(w.now(id).await["listening"], false);
}

#[sqlx::test]
async fn a_sender_without_metadata_is_passed_on_as_it_is(db: PgPool) {
    let w = world(db).await;
    w.sender.live_as("/ogg", "application/ogg", None, "x");
    let response = w
        .post(json!({"stream_url": w.sender.url("/ogg"), "name": "Ogg"}))
        .await;
    let id = body_json(response).await["id"].as_i64().unwrap();
    let uri = format!("/radio/stations/{id}/stream");
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(
        header_str(&response, header::CONTENT_TYPE),
        "application/ogg"
    );
    let mut body = response.into_body();
    let audio = read_bytes(&mut body, 2000).await;
    assert!(audio.iter().all(|&byte| byte == b'a'));
    // Somebody listens, but the sender names no title.
    assert_eq!(
        w.now(id).await,
        json!({"title": null, "artist": null, "raw": null, "listening": true})
    );
}

#[sqlx::test]
async fn a_title_without_a_clear_artist_stays_raw(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/news", "Nachrichten um 12");
    let response = w
        .post(json!({"stream_url": w.sender.url("/news"), "name": "Info"}))
        .await;
    let id = body_json(response).await["id"].as_i64().unwrap();
    let response = w
        .app
        .get(&format!("/radio/stations/{id}/stream"), Some(&w.token))
        .await;
    let mut body = response.into_body();
    read_bytes(&mut body, 64 * 3).await;
    assert_eq!(
        w.now(id).await,
        json!({"title": null, "artist": null, "raw": "Nachrichten um 12", "listening": true})
    );
}

#[sqlx::test]
async fn the_stream_takes_the_session_cookie_and_head_does_not_call_the_sender(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let uri = format!("/radio/stations/{id}/stream");
    let hits = w.sender.hits("/live");

    let head = Request::builder()
        .method(Method::HEAD)
        .uri(&uri)
        .header(header::AUTHORIZATION, format!("Bearer {}", w.token))
        .body(Body::empty())
        .unwrap();
    let response = w.app.send(head).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/mpeg");
    assert_eq!(w.sender.hits("/live"), hits);

    // The web player's <audio src> sends the cookie and nothing else.
    let cookie = format!("autoreverse_session={}", w.token);
    let response = w.app.get_with_cookie(&uri, &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    assert!(!read_bytes(&mut body, 64).await.is_empty());
    let stranger = w
        .app
        .get_with_cookie(&uri, "autoreverse_session=nope")
        .await;
    assert_eq!(stranger.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_sender_that_is_gone_or_changed_is_a_502(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let uri = format!("/radio/stations/{id}/stream");
    for (status, content_type) in [
        (503, "text/plain"),
        (200, "text/html"),
        (200, "audio/x-mpegurl"),
    ] {
        w.sender
            .fixed("/live", status, content_type, "<html>Sendepause</html>");
        let response = w.app.get(&uri, Some(&w.token)).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_GATEWAY,
            "{status} {content_type}"
        );
        assert_eq!(detail(response).await, "Sender nicht erreichbar");
    }
    // A stored address that points into the local network is not followed either — say, a row
    // from before the check existed, or a host name that moved.
    sqlx::query("UPDATE radio_stations SET stream_url = 'http://127.0.0.1:9/admin' WHERE id = $1")
        .bind(id as i32)
        .execute(w.app.db())
        .await
        .unwrap();
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    // A failed attempt holds no slot.
    assert_eq!(w.now(id).await["listening"], false);
}

#[sqlx::test]
async fn a_user_has_a_limited_number_of_streams_at_once(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let uri = format!("/radio/stations/{id}/stream");
    let mut open = Vec::new();
    for _ in 0..MAX_STREAMS_PER_USER {
        let response = w.app.get(&uri, Some(&w.token)).await;
        assert_eq!(response.status(), StatusCode::OK);
        open.push(response.into_body());
    }
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(header_str(&response, header::RETRY_AFTER), "5");
    assert_eq!(
        detail(response).await,
        "Zu viele Radio-Streams gleichzeitig"
    );
    // The refused one never reached the sender.
    eventually("all streams are connected", async || {
        w.sender.open_streams() == MAX_STREAMS_PER_USER
    })
    .await;

    // One ends, one may start.
    open.pop();
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

// --- Logos of search hits -----------------------------------------------------------------

const HIT_LOGO: &str = "/radio/stations/search/logo";

fn hit_id(n: u32) -> String {
    format!("bbbbbbbb-0000-0000-0000-{n:012}")
}

impl World {
    async fn hit_logo(&self, query: &str) -> Response<Body> {
        let uri = format!("{HIT_LOGO}?{query}");
        self.app.get(&uri, Some(&self.token)).await
    }

    /// The directory's answer to the next search, which is then made.
    async fn search(&self, entries: Value) -> Value {
        self.sender.json("/json/stations/search", entries);
        let uri = "/radio/stations/search?q=jazz";
        body_json(self.app.get(uri, Some(&self.token)).await).await
    }
}

async fn image_of(response: Response<Body>) -> image::DynamicImage {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    image::load_from_memory(&bytes).unwrap()
}

#[sqlx::test]
async fn the_logo_of_a_hit_comes_through_the_server_and_is_fetched_once(db: PgPool) {
    let w = world(db).await;
    let large = picture(600, ImageFormat::Png);
    w.sender
        .fixed("/large.png", 200, "image/png", large.clone());
    let with = w.sender.entry_with_logo(UUID, "/large.png");
    let without = w.sender.entry_with_logo(&hit_id(1), "");
    let found = w.search(json!([with, without])).await;
    assert_eq!(found["results"][0]["has_logo"], true);
    assert_eq!(found["results"][1]["has_logo"], false);

    let response = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(
        header_str(&response, header::CACHE_CONTROL),
        "private, max-age=86400"
    );
    assert!(response.headers().contains_key(header::ETAG));
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        large.as_slice()
    );

    // A list wants it small; the transparent corner stays transparent.
    let small = w.hit_logo(&format!("id={UUID}&size=100")).await;
    assert_eq!(small.status(), StatusCode::OK);
    assert_eq!(header_str(&small, header::CONTENT_TYPE), "image/png");
    let small = image_of(small).await.into_rgba8();
    assert_eq!((small.width(), small.height()), (128, 128));
    assert_eq!(small.get_pixel(127, 127)[3], 255);
    assert!(small.get_pixel(0, 0)[3] < 255);
    // Larger than the logo is the logo.
    let whole = w.hit_logo(&format!("id={UUID}&size=768")).await;
    assert_eq!(image_of(whole).await.width(), 600);

    // In any spelling of the id, with the session cookie as well as a token.
    let upper = w.hit_logo(&format!("id={}", UUID.to_uppercase())).await;
    assert_eq!(upper.status(), StatusCode::OK);
    let cookie = format!("autoreverse_session={}", w.token);
    let uri = format!("{HIT_LOGO}?id={UUID}&size=128");
    let by_cookie = w.app.get_with_cookie(&uri, &cookie).await;
    assert_eq!(by_cookie.status(), StatusCode::OK);
    // Somebody else's search made it known; the logo is no secret of one account.
    let other = w.app.user("bob", false).await;
    let other = w.app.token(other).await;
    assert_eq!(w.app.get(&uri, Some(&other)).await.status(), StatusCode::OK);

    // All of that was one request to the logo's server, and none to the directory by id.
    assert_eq!(w.sender.hits("/large.png"), 1);
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 0);

    // A hit without a logo has none, and nobody is asked about it.
    let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Kein Logo");
    let asked = format!("/json/stations/byuuid/{}", hit_id(1));
    assert_eq!(w.sender.hits(&asked), 0);

    let broken = w.hit_logo(&format!("id={UUID}&size=x")).await;
    assert_eq!(broken.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn the_logo_route_takes_an_id_and_nothing_else(db: PgPool) {
    let w = world(db).await;
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    let address = w.sender.url("/logo.png");
    let encoded = encode(&[("id", address.as_str())]);
    for query in [
        String::new(),
        "id=".into(),
        "id=../../logo.png".into(),
        encoded,
        encode(&[("url", address.as_str())]),
        encode(&[("logo_url", address.as_str()), ("id", "")]),
    ] {
        let response = w.hit_logo(&query).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{query}");
        assert_eq!(detail(response).await, "Kein Logo", "{query}");
    }
    assert!(w.sender.state.requests.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn an_id_the_server_does_not_know_is_asked_of_the_directory_once(db: PgPool) {
    let w = world(db).await;
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender.knows(&w.sender.entry_with_logo(UUID, "/logo.png"));
    for _ in 0..2 {
        let response = w.hit_logo(&format!("id={UUID}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            PNG.as_slice()
        );
    }
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 1);
    assert_eq!(w.sender.hits("/logo.png"), 1);

    // A station the directory does not know, one without a logo, and a directory that fails.
    let unknown = hit_id(1);
    let bare = hit_id(2);
    let failing = hit_id(3);
    w.sender
        .json(&format!("/json/stations/byuuid/{unknown}"), json!([]));
    w.sender.knows(&w.sender.entry_with_logo(&bare, ""));
    let path = format!("/json/stations/byuuid/{failing}");
    w.sender.fixed(&path, 500, "text/plain", "kaputt");
    for id in [&unknown, &bare, &failing] {
        for _ in 0..2 {
            let response = w.hit_logo(&format!("id={id}")).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
            assert_eq!(detail(response).await, "Kein Logo");
        }
        assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{id}")), 1);
    }
}

#[sqlx::test]
async fn what_is_no_usable_picture_is_a_404_and_is_not_fetched_again(db: PgPool) {
    let w = world(db).await;
    let png = picture(64, ImageFormat::Png);
    let mut too_large = png.clone();
    too_large.resize(2 * 1024 * 1024 + 1, 0);
    let cases: Vec<(&str, u16, &'static str, Vec<u8>)> = vec![
        ("/cut.png", 200, "image/png", png[..png.len() / 2].to_vec()),
        ("/signature.png", 200, "image/png", png[..8].to_vec()),
        (
            "/page.png",
            200,
            "image/png",
            b"<!doctype html><title>Nicht gefunden</title>".to_vec(),
        ),
        (
            "/logo.svg",
            200,
            "image/svg+xml",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>".to_vec(),
        ),
        ("/huge.png", 200, "image/png", too_large),
        ("/gone.png", 404, "image/png", png.clone()),
        ("/empty.png", 200, "image/png", Vec::new()),
    ];
    let mut entries = Vec::new();
    for (n, (path, status, content_type, body)) in cases.iter().enumerate() {
        w.sender.fixed(path, *status, content_type, body.clone());
        entries.push(w.sender.entry_with_logo(&hit_id(n as u32), path));
    }
    // Nothing at all answers under the last one's address.
    entries.push(w.sender.entry_with_logo(&hit_id(99), "/nothing.png"));
    w.search(Value::Array(entries)).await;

    let mut asked: Vec<(String, &str)> = Vec::new();
    for (n, case) in cases.iter().enumerate() {
        asked.push((hit_id(n as u32), case.0));
    }
    asked.push((hit_id(99), "/nothing.png"));
    for (id, path) in &asked {
        for query in [format!("id={id}"), format!("id={id}&size=128")] {
            let response = w.hit_logo(&query).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert_eq!(detail(response).await, "Kein Logo", "{path}");
        }
        // The failure is remembered: the dead address was tried once.
        assert_eq!(w.sender.hits(path), 1, "{path}");
    }
}

#[sqlx::test]
async fn a_picture_under_a_wrong_name_or_type_is_still_a_logo(db: PgPool) {
    let w = world(db).await;
    // What the sender calls it does not count either way: the content does.
    let jpeg = {
        let image = image::RgbImage::from_pixel(32, 32, image::Rgb([10, 120, 200]));
        let mut data = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut data), ImageFormat::Jpeg)
            .unwrap();
        data
    };
    w.sender.fixed("/favicon.ico", 200, "image/x-icon", jpeg);
    w.sender.fixed("/logo", 200, "text/html", PNG.as_slice());
    w.sender
        .fixed("/real.ico", 200, "", picture(48, ImageFormat::Ico));
    w.search(json!([
        w.sender.entry_with_logo(&hit_id(1), "/favicon.ico"),
        w.sender.entry_with_logo(&hit_id(2), "/logo"),
        w.sender.entry_with_logo(&hit_id(3), "/real.ico"),
    ]))
    .await;
    for (n, content_type) in [(1, "image/jpeg"), (2, "image/png"), (3, "image/png")] {
        let response = w.hit_logo(&format!("id={}", hit_id(n))).await;
        assert_eq!(response.status(), StatusCode::OK, "{n}");
        assert_eq!(header_str(&response, header::CONTENT_TYPE), content_type);
        assert!(response.headers().contains_key("x-content-type-options"));
    }
    // The icon file went out as a PNG of its picture.
    let icon = image_of(w.hit_logo(&format!("id={}", hit_id(3))).await).await;
    assert_eq!((icon.width(), icon.height()), (48, 48));
    assert_eq!(icon.into_rgba8().get_pixel(0, 0)[3], 0);
}

#[sqlx::test]
async fn a_logo_server_that_does_not_answer_in_time_costs_one_wait(db: PgPool) {
    let timing = Timing {
        fetch: Duration::from_millis(200),
        keep_failure: Duration::from_millis(600),
        ..Timing::default()
    };
    let w = world_timed(db, &[], timing).await;
    w.sender.slow("/logo.png", Duration::from_secs(20));
    w.search(json!([w.sender.entry_with_logo(UUID, "/logo.png")]))
        .await;

    let started = std::time::Instant::now();
    let response = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(started.elapsed() < Duration::from_secs(5));

    // While that is remembered, nobody waits again — even with the server back.
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    let started = std::time::Instant::now();
    let response = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(started.elapsed() < Duration::from_millis(150));
    assert_eq!(w.sender.hits("/logo.png"), 1);

    // It is not remembered for ever.
    tokio::time::sleep(Duration::from_millis(650)).await;
    let response = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(w.sender.hits("/logo.png"), 2);
}

#[sqlx::test]
async fn requests_for_the_same_logo_share_one_fetch(db: PgPool) {
    let w = world(db).await;
    w.sender.slow("/logo.png", Duration::from_millis(150));
    w.sender.knows(&w.sender.entry_with_logo(UUID, "/logo.png"));
    let query = format!("id={UUID}&size=128");
    let responses =
        futures_util::future::join_all((0..8).map(|_| async { w.hit_logo(&query).await })).await;
    for response in responses {
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(w.sender.hits("/logo.png"), 1);
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 1);
}

#[sqlx::test]
async fn a_client_that_hangs_up_does_not_undo_the_fetch(db: PgPool) {
    let w = world(db).await;
    w.sender.slow("/logo.png", Duration::from_millis(200));
    w.search(json!([w.sender.entry_with_logo(UUID, "/logo.png")]))
        .await;
    let query = format!("id={UUID}");
    let impatient = tokio::time::timeout(Duration::from_millis(50), w.hit_logo(&query)).await;
    assert!(impatient.is_err());
    let response = w.hit_logo(&query).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(w.sender.hits("/logo.png"), 1);
}

#[sqlx::test]
async fn a_logo_in_the_local_network_is_not_fetched(db: PgPool) {
    let w = world(db).await;
    // Another server on this machine: not the directory's host and port, so the fence holds.
    let inside = Sender::start().await;
    inside.fixed("/logo.png", 200, "image/png", PNG.as_slice());
    let port = inside.base.rsplit(':').next().unwrap().to_owned();
    let addresses = [
        inside.url("/logo.png"),
        format!("http://localhost:{port}/logo.png"),
        format!("http://[::1]:{port}/logo.png"),
        format!("http://0.0.0.0:{port}/logo.png"),
        "http://192.168.1.1/logo.png".to_owned(),
        "http://10.0.0.1/logo.png".to_owned(),
        "http://169.254.169.254/latest/meta-data/".to_owned(),
        "file:///etc/passwd".to_owned(),
    ];
    let entries: Vec<Value> = addresses
        .iter()
        .enumerate()
        .map(|(n, address)| w.sender.entry_with_logo(&hit_id(n as u32), address))
        .collect();
    w.search(Value::Array(entries)).await;
    for (n, address) in addresses.iter().enumerate() {
        let started = std::time::Instant::now();
        let response = w.hit_logo(&format!("id={}", hit_id(n as u32))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{address}");
        // Refused, not tried until a timeout.
        assert!(started.elapsed() < Duration::from_secs(2), "{address}");
    }
    assert!(inside.state.requests.lock().unwrap().is_empty());

    // The same goes for the logo of a station that is added.
    w.sender.live("/s", "x");
    let entry = w.sender.entry_with_logo(UUID, &inside.url("/logo.png"));
    w.sender.knows(&entry);
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(body_json(response).await["has_logo"], false);
    assert!(inside.state.requests.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn a_station_keeps_the_logo_its_hit_showed_also_from_an_icon_file(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/s", "x");
    w.sender.fixed(
        "/favicon.ico",
        200,
        "image/x-icon",
        picture(64, ImageFormat::Ico),
    );
    let entry = w.sender.entry_with_logo(UUID, "/favicon.ico");
    w.sender.knows(&entry);
    w.search(json!([entry])).await;
    let shown = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(shown.status(), StatusCode::OK);
    let shown = shown.into_body().collect().await.unwrap().to_bytes();

    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(station["has_logo"], true);
    let id = station["id"].as_i64().unwrap();
    // Stored as the PNG that was shown, without asking the logo's server again.
    let file = w.app.state.config.radio_dir.join(format!("logos/{id}.png"));
    assert_eq!(std::fs::read(&file).unwrap(), shown);
    assert_eq!(w.sender.hits("/favicon.ico"), 1);
    let uri = format!("/radio/stations/{id}/logo");
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(image_of(response).await.width(), 64);
}

#[sqlx::test]
async fn a_stored_station_without_a_logo_gets_it_when_the_list_is_read(db: PgPool) {
    // No memory for failures, so the second try is not answered from the first.
    let timing = Timing {
        keep_failure: Duration::ZERO,
        ..Timing::default()
    };
    let w = world_timed(db, &[], timing).await;
    w.sender.live("/s", "x");
    // As it was before icon files were understood: the station is there, the logo is not.
    w.sender
        .knows(&w.sender.entry_with_logo(UUID, "/favicon.ico"));
    let station = body_json(w.post(json!({"directory_id": UUID})).await).await;
    assert_eq!(station["has_logo"], false);
    let by_address = w.station("/plain", "Schlicht").await;
    w.sender.fixed(
        "/favicon.ico",
        200,
        "image/x-icon",
        picture(32, ImageFormat::Ico),
    );

    // The list answers at once and starts the catching up.
    let asked_before = w.sender.hits(&format!("/json/stations/byuuid/{UUID}"));
    let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
    assert_eq!(list["stations"].as_array().unwrap().len(), 2);
    eventually("the logo arrives", async || {
        let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
        list["stations"][0]["has_logo"] == true
    })
    .await;
    let uri = format!("/radio/stations/{}/logo", station["id"]);
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(image_of(response).await.width(), 32);
    // A station that was added by its address has no directory to ask.
    let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
    assert_eq!(list["stations"][1]["id"], by_address);
    assert_eq!(list["stations"][1]["has_logo"], false);
    // The directory was not asked again: what it said when the station was added still holds.
    assert_eq!(
        w.sender.hits(&format!("/json/stations/byuuid/{UUID}")),
        asked_before
    );
}

#[sqlx::test]
async fn a_logo_that_stays_away_is_not_tried_with_every_list(db: PgPool) {
    let timing = Timing {
        keep_failure: Duration::ZERO,
        ..Timing::default()
    };
    let w = world_timed(db, &[], timing).await;
    w.sender.live("/s", "x");
    w.sender.knows(&w.sender.entry_with_logo(UUID, "/gone.png"));
    w.post(json!({"directory_id": UUID})).await;
    assert_eq!(w.sender.hits("/gone.png"), 1);

    w.app.get("/radio/stations", Some(&w.token)).await;
    eventually("the second try", async || w.sender.hits("/gone.png") == 2).await;
    for _ in 0..3 {
        w.app.get("/radio/stations", Some(&w.token)).await;
        page(&w, "/library/radio").await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(w.sender.hits("/gone.png"), 2);
}

#[sqlx::test]
async fn without_the_directory_no_logo_is_caught_up_on(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO_DIRECTORY", "0")]).await;
    let id = w.station("/a", "A").await;
    // A station from the days when the directory was still asked.
    sqlx::query("UPDATE radio_stations SET directory_id = $2 WHERE id = $1")
        .bind(id as i32)
        .bind(UUID)
        .execute(w.app.db())
        .await
        .unwrap();
    w.sender.knows(&w.sender.entry_with_logo(UUID, "/logo.png"));
    w.app.get("/radio/stations", Some(&w.token)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 0);
    assert_eq!(w.sender.hits("/logo.png"), 0);
}

// --- Logos from a station's homepage ------------------------------------------------------

const TOUCH: &str = r#"<link rel="apple-touch-icon" href="/touch.png">"#;

fn hit_logo_bytes(response: Response<Body>) -> impl Future<Output = Bytes> {
    assert_eq!(response.status(), StatusCode::OK);
    async { response.into_body().collect().await.unwrap().to_bytes() }
}

#[sqlx::test]
async fn a_hit_without_a_directory_logo_shows_the_icon_of_its_homepage(db: PgPool) {
    let w = world(db).await;
    let touch = picture(180, ImageFormat::Png);
    w.sender
        .fixed("/touch.png", 200, "image/png", touch.clone());
    w.sender
        .fixed("/icon.png", 200, "image/png", picture(48, ImageFormat::Png));
    let head = format!(r#"<link rel="icon" href="/icon.png">{TOUCH}"#);
    w.sender.page("/sender/", &head);

    let found = w
        .search(json!([
            w.sender.entry_with_page(&hit_id(1), "", "/sender/"),
            w.sender.entry_with_page(&hit_id(2), "", "/sender/"),
            w.sender.entry_with_page(&hit_id(3), "", ""),
        ]))
        .await;
    // "Worth asking for": no logo in the directory, but a homepage.
    for (n, has_logo) in [(0, true), (1, true), (2, false)] {
        assert_eq!(found["results"][n]["logo_url"], Value::Null);
        assert_eq!(found["results"][n]["has_logo"], has_logo, "{n}");
    }
    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(html.contains(&format!("search/logo?id={}&amp;", hit_id(1))));
    assert!(!html.contains(&format!("search/logo?id={}&amp;", hit_id(3))));
    // Searching — through the API or the page — asked no homepage for anything.
    assert_eq!(w.sender.hits("/sender/"), 0);
    assert_eq!(w.sender.hits("/touch.png"), 0);

    let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(hit_logo_bytes(response).await, touch.as_slice());
    let small = w.hit_logo(&format!("id={}&size=128", hit_id(1))).await;
    assert_eq!(image_of(small).await.width(), 128);
    // The touch icon was enough: the lesser icon was never asked for.
    assert_eq!(w.sender.hits("/sender/"), 1);
    assert_eq!(w.sender.hits("/touch.png"), 1);
    assert_eq!(w.sender.hits("/icon.png"), 0);

    // Another channel of the same broadcaster: the page is not read a second time.
    let response = w.hit_logo(&format!("id={}", hit_id(2))).await;
    assert_eq!(hit_logo_bytes(response).await, touch.as_slice());
    assert_eq!(w.sender.hits("/sender/"), 1);
    assert_eq!(w.sender.hits("/touch.png"), 1);

    let response = w.hit_logo(&format!("id={}", hit_id(3))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_homepage_is_asked_only_when_the_directory_logo_fails(db: PgPool) {
    let w = world(db).await;
    let touch = picture(180, ImageFormat::Png);
    w.sender
        .fixed("/touch.png", 200, "image/png", touch.clone());
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender
        .fixed("/logo.svg", 200, "image/svg+xml", "<svg xmlns=\"x\"/>");
    for path in ["/works/", "/gone/", "/vector/"] {
        w.sender.page(path, TOUCH);
    }
    w.search(json!([
        w.sender.entry_with_page(&hit_id(1), "/logo.png", "/works/"),
        w.sender.entry_with_page(&hit_id(2), "/gone.png", "/gone/"),
        w.sender
            .entry_with_page(&hit_id(3), "/logo.svg", "/vector/"),
    ]))
    .await;

    // The directory's logo, small as it is, is the station's own choice.
    let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
    assert_eq!(hit_logo_bytes(response).await, PNG.as_slice());
    assert_eq!(w.sender.hits("/works/"), 0);
    assert_eq!(w.sender.hits("/touch.png"), 0);

    // One that is gone, and one that is no picture the server passes on.
    for (n, logo, page) in [(2, "/gone.png", "/gone/"), (3, "/logo.svg", "/vector/")] {
        let response = w.hit_logo(&format!("id={}", hit_id(n))).await;
        assert_eq!(hit_logo_bytes(response).await, touch.as_slice(), "{logo}");
        assert_eq!(w.sender.hits(logo), 1);
        assert_eq!(w.sender.hits(page), 1);
    }
}

#[sqlx::test]
async fn a_page_that_names_no_icon_is_asked_for_the_usual_ones(db: PgPool) {
    let w = world(db).await;
    w.sender.fixed(
        "/favicon.ico",
        200,
        "image/x-icon",
        picture(48, ImageFormat::Ico),
    );
    w.sender.page("/plain/", "");
    w.sender
        .fixed("/broken/", 200, "text/html", "<html><head><link rel=icon");
    w.sender.fixed("/error/", 503, "text/html", "Wartung");
    // A homepage field that names the stream.
    w.sender.live("/stream", "x");
    let pages = ["/plain/", "/broken/", "/error/", "/missing/", "/stream"];
    let entries: Vec<Value> = pages
        .iter()
        .enumerate()
        .map(|(n, page)| w.sender.entry_with_page(&hit_id(n as u32), "", page))
        .collect();
    w.search(Value::Array(entries)).await;
    for (n, page) in pages.iter().enumerate() {
        let response = w.hit_logo(&format!("id={}", hit_id(n as u32))).await;
        assert_eq!(response.status(), StatusCode::OK, "{page}");
        // The icon file's picture, as a PNG.
        assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
        assert_eq!(image_of(response).await.width(), 48, "{page}");
        assert_eq!(w.sender.hits(page), 1, "{page}");
    }
    assert_eq!(w.sender.hits("/apple-touch-icon.png"), pages.len());
    assert_eq!(w.sender.open_streams(), 0);
}

#[sqlx::test]
async fn links_count_from_where_the_page_was_really_served(db: PgPool) {
    let w = world(db).await;
    let touch = picture(120, ImageFormat::Png);
    w.sender
        .fixed("/de/start/img/touch.png", 200, "image/png", touch.clone());
    w.sender
        .fixed("/cdn/based.png", 200, "image/png", touch.clone());
    w.sender.moved("/old", "/de/start/");
    w.sender.page(
        "/de/start/",
        r#"<link rel="apple-touch-icon" href="img/touch.png?v=1&amp;w=2">"#,
    );
    w.sender.page(
        "/based/",
        r#"<base href="/cdn/"><link rel="apple-touch-icon" href="based.png">"#,
    );
    w.search(json!([
        w.sender.entry_with_page(&hit_id(1), "", "/old"),
        w.sender.entry_with_page(&hit_id(2), "", "/based/"),
    ]))
    .await;
    for n in [1, 2] {
        let response = w.hit_logo(&format!("id={}", hit_id(n))).await;
        assert_eq!(hit_logo_bytes(response).await, touch.as_slice(), "{n}");
    }
    let (asked, _) = w.sender.last_request("/de/start/img/touch.png");
    assert_eq!(asked, "/de/start/img/touch.png?v=1&w=2");
    assert_eq!(w.sender.hits("/cdn/based.png"), 1);
}

#[sqlx::test]
async fn a_tiny_icon_is_taken_only_when_nothing_better_can_be_had(db: PgPool) {
    let w = world(db).await;
    let tiny = picture(16, ImageFormat::Png);
    w.sender.fixed("/tiny.png", 200, "image/png", tiny.clone());
    let photo = picture(300, ImageFormat::Png);
    w.sender
        .fixed("/studio.png", 200, "image/png", photo.clone());
    let icon = r#"<link rel="shortcut icon" href="/tiny.png">"#;
    let social = r#"<meta property="og:image" content="/studio.png">"#;
    w.sender.page("/only/", icon);
    w.sender.page("/social/", &format!("{social}{icon}"));
    w.sender.page("/later/", icon);
    w.search(json!([
        w.sender.entry_with_page(&hit_id(1), "", "/only/"),
        w.sender.entry_with_page(&hit_id(2), "", "/social/"),
        w.sender.entry_with_page(&hit_id(3), "", "/later/"),
    ]))
    .await;

    // Nothing else there: sixteen pixels are better than no logo.
    let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
    assert_eq!(hit_logo_bytes(response).await, tiny.as_slice());
    assert_eq!(w.sender.hits("/apple-touch-icon.png"), 1);
    assert_eq!(w.sender.hits("/favicon.ico"), 1);
    // The picture for social networks is the last resort, but it beats a blur.
    let response = w.hit_logo(&format!("id={}", hit_id(2))).await;
    assert_eq!(hit_logo_bytes(response).await, photo.as_slice());
    // And so does what browsers find unasked.
    w.sender.fixed(
        "/favicon.ico",
        200,
        "image/x-icon",
        picture(64, ImageFormat::Ico),
    );
    let response = w.hit_logo(&format!("id={}", hit_id(3))).await;
    assert_eq!(image_of(response).await.width(), 64);
}

#[sqlx::test]
async fn a_page_costs_a_bounded_number_of_requests_and_a_bounded_time(db: PgPool) {
    let timing = Timing {
        fetch: Duration::from_millis(300),
        site: Duration::from_millis(500),
        ..Timing::default()
    };
    let w = world_timed(db, &[], timing).await;
    // Seven links, each to something that is no logo; the page counts as a request as well.
    let paths: Vec<String> = (0..7).map(|n| format!("/no/{n}.png")).collect();
    let mut head = String::new();
    for (n, path) in paths.iter().enumerate() {
        let edge = 700 - n;
        head += &format!(r#"<link rel="icon" sizes="{edge}x{edge}" href="{path}">"#);
        w.sender
            .fixed(path, 200, "image/png", "<html>nichts</html>");
    }
    w.sender.page("/many/", &head);
    // A page whose icons do not answer: the second one gets what time is left, the third none.
    for path in ["/slow/1.png", "/slow/2.png", "/slow/3.png"] {
        w.sender.slow(path, Duration::from_secs(20));
    }
    w.sender.page(
        "/slow/",
        r#"<link rel="apple-touch-icon" sizes="180x180" href="/slow/1.png">
           <link rel="apple-touch-icon" sizes="120x120" href="/slow/2.png">
           <link rel="apple-touch-icon" sizes="60x60" href="/slow/3.png">"#,
    );
    w.search(json!([
        w.sender.entry_with_page(&hit_id(1), "", "/many/"),
        w.sender.entry_with_page(&hit_id(2), "", "/slow/"),
    ]))
    .await;

    for _ in 0..2 {
        let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let asked: Vec<usize> = paths.iter().map(|path| w.sender.hits(path)).collect();
    assert_eq!(asked, [1, 1, 1, 1, 0, 0, 0]);
    assert_eq!(w.sender.hits("/favicon.ico"), 0);
    // The failure is remembered like any other: the page was read once.
    assert_eq!(w.sender.hits("/many/"), 1);

    let started = std::time::Instant::now();
    let response = w.hit_logo(&format!("id={}", hit_id(2))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(started.elapsed() >= Duration::from_millis(450));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(w.sender.hits("/slow/1.png"), 1);
    assert_eq!(w.sender.hits("/slow/2.png"), 1);
    assert_eq!(w.sender.hits("/slow/3.png"), 0);
}

#[sqlx::test]
async fn a_page_cannot_point_the_server_into_the_local_network(db: PgPool) {
    let w = world(db).await;
    // Another server on this machine: not the directory's host and port, so the fence holds.
    let inside = Sender::start().await;
    inside.fixed("/secret.png", 200, "image/png", PNG.as_slice());
    inside.page("/", TOUCH);
    let port = inside.base.rsplit(':').next().unwrap().to_owned();
    let good = picture(64, ImageFormat::Png);
    w.sender.fixed("/good.png", 200, "image/png", good.clone());
    let head = format!(
        r#"<link rel="apple-touch-icon" href="{}">
           <link rel="icon" sizes="512x512" href="http://localhost:{port}/secret.png">
           <link rel="icon" sizes="256x256" href="//192.168.1.1/secret.png">
           <link rel="icon" sizes="64x64" href="/good.png">
           <meta property="og:image" content="http://169.254.169.254/latest/meta-data/">"#,
        inside.url("/secret.png")
    );
    w.sender.page("/linked/", &head);
    w.sender.page(
        "/based/",
        &format!(
            r#"<base href="{}/"><link rel="icon" href="secret.png">"#,
            inside.base
        ),
    );
    w.search(json!([
        w.sender.entry_with_page(&hit_id(1), "", "/linked/"),
        w.sender.entry_with_page(&hit_id(2), "", "/based/"),
        // A homepage that is itself inside.
        w.sender.entry_with_page(&hit_id(3), "", &inside.url("/")),
        w.sender
            .entry_with_page(&hit_id(4), "", &format!("http://localhost:{port}/")),
        w.sender.entry_with_page(&hit_id(5), "", "http://10.0.0.1/"),
    ]))
    .await;

    // The links into the local network are passed over, the one that may be fetched is.
    let started = std::time::Instant::now();
    let response = w.hit_logo(&format!("id={}", hit_id(1))).await;
    assert_eq!(hit_logo_bytes(response).await, good.as_slice());
    for n in 2..=5 {
        let response = w.hit_logo(&format!("id={}", hit_id(n))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{n}");
    }
    // Refused, not tried until a timeout.
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(inside.state.requests.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn adding_a_station_takes_the_icon_of_its_homepage(db: PgPool) {
    let w = world(db).await;
    let touch = picture(180, ImageFormat::Png);
    w.sender
        .fixed("/touch.png", 200, "image/png", touch.clone());
    w.sender.page("/sender/", TOUCH);
    w.sender.page("/other/", TOUCH);
    w.sender.live("/s", "x");
    let stored = |id: i64| {
        let file = w.app.state.config.radio_dir.join(format!("logos/{id}.png"));
        std::fs::read(file).unwrap()
    };

    // From the directory, which names a dead logo: the hit showed the homepage's icon, and
    // that is what the station keeps — without asking the page again.
    let entry = w.sender.entry_with_page(UUID, "/gone.png", "/sender/");
    w.sender.knows(&entry);
    w.search(json!([entry])).await;
    let shown = w.hit_logo(&format!("id={UUID}")).await;
    assert_eq!(hit_logo_bytes(shown).await, touch.as_slice());
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(station["has_logo"], true);
    assert_eq!(stored(station["id"].as_i64().unwrap()), touch);
    assert_eq!(w.sender.hits("/sender/"), 1);
    assert_eq!(w.sender.hits("/touch.png"), 1);

    // Straight from the directory, without a search before, and with no logo named at all.
    let bare = w.sender.entry_with_page(&hit_id(1), "", "/other/");
    w.sender.live("/t", "x");
    let mut bare = bare;
    bare["url"] = json!(w.sender.url("/t"));
    bare["url_resolved"] = json!(w.sender.url("/t"));
    w.sender.knows(&bare);
    let station = body_json(w.post(json!({"directory_id": hit_id(1)})).await).await;
    assert_eq!(station["has_logo"], true);
    assert_eq!(stored(station["id"].as_i64().unwrap()), touch);

    // By its address, with the homepage the user named.
    w.sender.live("/u", "x");
    let body = json!({"stream_url": w.sender.url("/u"), "name": "Eigener",
                      "homepage": w.sender.url("/sender/")});
    let response = w.post(body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(station["has_logo"], true);
    let id = station["id"].as_i64().unwrap();
    assert_eq!(stored(id), touch);
    let response = w
        .app
        .get(&format!("/radio/stations/{id}/logo"), Some(&w.token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // The page was in memory from the hit.
    assert_eq!(w.sender.hits("/sender/"), 1);
}

#[sqlx::test]
async fn a_station_added_by_address_has_its_logo_also_without_the_directory(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO_DIRECTORY", "0")]).await;
    w.sender.fixed(
        "/touch.png",
        200,
        "image/png",
        picture(180, ImageFormat::Png),
    );
    w.sender.page("/sender/", TOUCH);
    w.sender.live("/u", "x");
    let body = json!({"stream_url": w.sender.url("/u"), "name": "Eigener",
                      "homepage": w.sender.url("/sender/")});
    let station = body_json(w.post(body).await).await;
    assert_eq!(station["has_logo"], true);
    // A homepage that offers nothing leaves the station without a logo, but added.
    w.sender.live("/v", "x");
    let body = json!({"stream_url": w.sender.url("/v"), "name": "Ohne",
                      "homepage": w.sender.url("/nichts/")});
    let response = w.post(body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(body_json(response).await["has_logo"], false);
    assert_eq!(w.sender.hits("/nichts/"), 1);
}

#[sqlx::test]
async fn stored_stations_without_a_logo_get_their_homepages_icon_with_the_list(db: PgPool) {
    // No memory for failures, so the second try is not answered from the first.
    let timing = Timing {
        keep_failure: Duration::ZERO,
        ..Timing::default()
    };
    let w = world_timed(db, &[], timing).await;
    w.sender.live("/s", "x");
    w.sender.live("/u", "x");
    // Both were added while their pages had nothing to offer.
    w.sender
        .knows(&w.sender.entry_with_page(UUID, "", "/sender/"));
    let listed = body_json(w.post(json!({"directory_id": UUID})).await).await;
    let body = json!({"stream_url": w.sender.url("/u"), "name": "Eigener",
                      "homepage": w.sender.url("/eigener/")});
    let own = body_json(w.post(body).await).await;
    assert_eq!(
        (&listed["has_logo"], &own["has_logo"]),
        (&json!(false), &json!(false))
    );

    w.sender.fixed(
        "/touch.png",
        200,
        "image/png",
        picture(180, ImageFormat::Png),
    );
    w.sender.page("/sender/", TOUCH);
    w.sender.page("/eigener/", TOUCH);
    let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
    assert_eq!(list["stations"].as_array().unwrap().len(), 2);
    eventually("the logos arrive", async || {
        let list = body_json(w.app.get("/radio/stations", Some(&w.token)).await).await;
        list["stations"][0]["has_logo"] == true && list["stations"][1]["has_logo"] == true
    })
    .await;
    for station in [&listed, &own] {
        let uri = format!("/radio/stations/{}/logo", station["id"]);
        let response = w.app.get(&uri, Some(&w.token)).await;
        assert_eq!(image_of(response).await.width(), 180);
    }
}

// --- Switches -----------------------------------------------------------------------------

#[sqlx::test]
async fn without_the_feature_the_routes_do_not_exist(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO", "0")]).await;
    let off = "Radio ist auf diesem Server abgeschaltet";
    for token in [Some(w.token.as_str()), None] {
        for uri in [
            "/radio/stations",
            "/radio/stations/search?q=jazz",
            "/radio/stations/search/logo?id=d28420a4-eccf-47a2-ace1-088c7e7cb7e0",
            "/radio/stations/1/stream",
            "/radio/stations/1/now",
            "/radio/stations/1/logo",
        ] {
            let response = w.app.get(uri, token).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(detail(response).await, off, "{uri}");
        }
        let response = w
            .app
            .post_json("/radio/stations", json!({"directory_id": UUID}), token)
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        for method in [Method::PATCH, Method::DELETE] {
            let response = w
                .app
                .call(method, "/radio/stations/1", Some(json!({})), token)
                .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }
    let info = body_json(w.app.get("/server/info", Some(&w.token)).await).await;
    assert_eq!(info["radio"], false);
    assert_eq!(info["radio_directory"], false);
    // The Titel-Radio is another thing and stays.
    let response = w.app.get("/radio?artist=Niemand", Some(&w.token)).await;
    assert_ne!(detail(response).await, off);
}

#[sqlx::test]
async fn without_the_directory_stations_are_added_by_address_only(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO_DIRECTORY", "0")]).await;
    let off = "Die Sendersuche ist auf diesem Server abgeschaltet";
    w.sender.json("/json/stations/search", json!([]));
    w.sender.knows(&w.sender.entry(UUID, "Jazz Eins", "/jazz"));

    let response = w
        .app
        .get("/radio/stations/search?q=jazz", Some(&w.token))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, off);
    let response = w.post(json!({"directory_id": UUID})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, off);
    let uri = format!("/radio/stations/search/logo?id={UUID}");
    let response = w.app.get(&uri, Some(&w.token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, off);
    // Nobody was asked.
    assert_eq!(w.sender.hits("/json/stations/search"), 0);
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 0);

    w.station("/a", "A").await;
    assert_eq!(w.names().await, ["A"]);
    let info = body_json(w.app.get("/server/info", Some(&w.token)).await).await;
    assert_eq!(info["radio"], true);
    assert_eq!(info["radio_directory"], false);
}

#[sqlx::test]
async fn the_server_info_names_both_switches(db: PgPool) {
    let w = world(db).await;
    let info = body_json(w.app.get("/server/info", Some(&w.token)).await).await;
    assert_eq!(info["radio"], true);
    assert_eq!(info["radio_directory"], true);
}

#[sqlx::test]
async fn listening_to_a_station_is_booked_nowhere(db: PgPool) {
    let w = world(db).await;
    let id = w.station("/live", "Live").await;
    let response = w
        .app
        .get(&format!("/radio/stations/{id}/stream"), Some(&w.token))
        .await;
    let mut body = response.into_body();
    read_bytes(&mut body, 64 * 4).await;
    drop(body);
    let (events, progress, sessions): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM listen_events), (SELECT count(*) FROM progress),
                (SELECT count(*) FROM playback_sessions)",
    )
    .fetch_one(w.app.db())
    .await
    .unwrap();
    assert_eq!((events, progress, sessions), (0, 0, 0));
}

// --- The web page -------------------------------------------------------------------------

async fn page(w: &World, uri: &str) -> String {
    let cookie = format!("autoreverse_session={}", w.token);
    let response = w.app.get_with_cookie(uri, &cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

async fn form(w: &World, uri: &str, form: &str) -> Response<Body> {
    let cookie = format!("autoreverse_session={}", w.token);
    w.app.post_form(uri, form, Some(&cookie)).await
}

fn encode(pairs: &[(&str, &str)]) -> String {
    serde_urlencoded::to_string(pairs).unwrap()
}

#[sqlx::test]
async fn the_page_shows_the_stations_as_tiles_that_play(db: PgPool) {
    let w = world(db).await;
    // Without stations: the invitation, and the navigation entries.
    let html = page(&w, "/library/radio").await;
    assert!(html.contains("<title>Radio · Autoreverse</title>"));
    assert!(html.contains(
        "Noch keine Sender. Unten nach einem Sender suchen oder seine Adresse eintragen."
    ));
    assert!(html.contains(r#"<a href="/library/radio" aria-current="true">Radio</a>"#));
    assert!(
        html.contains(r#"<a href="/library/radio" class="pill" aria-current="true">Radio</a>"#)
    );
    assert!(html.contains(r#"<link rel="stylesheet" href="/css/pages/radio.css" />"#));
    // Other pages link to it.
    let albums = page(&w, "/library/albums").await;
    assert!(albums.contains(r#"<a href="/library/radio" aria-current="false">Radio</a>"#));

    w.sender.live("/jazz", "x");
    w.sender
        .fixed("/logo.png", 200, "image/png", PNG.as_slice());
    w.sender
        .knows(&w.sender.entry(UUID, "Jazz & Eins", "/jazz"));
    let with_logo = body_json(w.post(json!({"directory_id": UUID})).await).await["id"]
        .as_i64()
        .unwrap();
    let plain = w.station("/plain", "Schlicht").await;

    let html = page(&w, "/library/radio").await;
    assert!(!html.contains("Noch keine Sender"));
    // The tile carries what the player needs, and its button plays.
    assert!(html.contains(&format!(
        r#"<div class="tile station" data-station-id="{with_logo}" data-station-name="Jazz &#38; Eins" data-station-logo="true" aria-current="false">"#
    )));
    assert!(html.contains(
        r#"<button type="button" class="logo" data-action="play-station" aria-label="Jazz &#38; Eins live hören" aria-pressed="false" title="Live hören">"#
    ));
    assert!(html.contains(&format!(
        r#"<img src="/radio/stations/{with_logo}/logo?size=256" alt="" loading="lazy" />"#
    )));
    assert!(html.contains(r#"<span class="subtitle">DE · MP3 · 192 kbit/s</span>"#));
    assert!(html.contains(&format!(
        r#"data-station-id="{plain}" data-station-name="Schlicht" data-station-logo="false""#
    )));
    assert!(html.contains(r#"<span class="subtitle">MP3 · 128 kbit/s</span>"#));
    // In the user's order.
    assert!(html.find("Jazz &#38; Eins").unwrap() < html.find("Schlicht").unwrap());
    // Rename and remove.
    assert!(html.contains(&format!(
        r#"<form method="POST" action="/library/radio/{plain}/rename">"#
    )));
    assert!(html.contains(&format!(
        r#"<form method="POST" action="/library/radio/{plain}/delete" hx-confirm="Sender „Schlicht“ entfernen?">"#
    )));
    // The address of a stream is nobody's business in the browser.
    assert!(!html.contains(&w.sender.base));
}

#[sqlx::test]
async fn the_page_searches_the_directory_and_adds_a_hit(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/jazz", "x");
    let entry = w.sender.entry(UUID, "Jazz Eins", "/jazz");
    w.sender.knows(&entry);
    w.sender.json("/json/stations/search", json!([entry]));

    // Nothing is asked before somebody searches.
    let html = page(&w, "/library/radio").await;
    assert!(html.contains(r#"placeholder="Sender suchen, z. B. Jazz""#));
    assert!(!html.contains("station-results"));
    assert_eq!(w.sender.hits("/json/stations/search"), 0);

    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(html.contains(r#"<input name="q" value="jazz""#));
    assert!(html.contains(r#"<span class="name">Jazz Eins</span>"#));
    assert!(
        html.contains(r#"<span class="facts">DE · MP3 · 192 kbit/s · jazz, smooth jazz</span>"#)
    );
    assert!(html.contains(&format!(
        r#"<input type="hidden" name="directory_id" value="{UUID}" />"#
    )));
    assert!(html.contains(r#"aria-label="Jazz Eins hinzufügen">Hinzufügen</button>"#));
    // No logo is loaded from the directory's word by the browser: it comes through this
    // server, by the hit's id.
    assert!(!html.contains(&w.sender.base));
    assert!(html.contains(&format!(
        r#"<span class="hit-logo" aria-hidden="true"><img src="/radio/stations/search/logo?id={UUID}&amp;size=128" alt="" loading="lazy" data-fallback /><svg class="icon""#
    )));
    let mut bare = entry.clone();
    bare["favicon"] = json!("");
    bare["homepage"] = json!("");
    w.sender.json("/json/stations/search", json!([bare]));
    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(html.contains(r#"<span class="hit-logo" aria-hidden="true"><svg class="icon""#));
    assert!(!html.contains("/radio/stations/search/logo"));
    w.sender.json("/json/stations/search", json!([entry]));

    // "Hinzufügen" leads back to the same search, where the hit is now marked.
    let response = form(
        &w,
        "/library/radio",
        &encode(&[("directory_id", UUID), ("q", "jazz")]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, header::LOCATION),
        "/library/radio?q=jazz"
    );
    assert_eq!(w.names().await, ["Jazz Eins"]);
    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(html.contains(r#"<span class="pill added">Hinzugefügt</span>"#));
    assert!(!html.contains(r#"name="directory_id""#));

    // No hits, and a directory that is down.
    w.sender.json("/json/stations/search", json!([]));
    assert!(
        page(&w, "/library/radio?q=zzz")
            .await
            .contains(r#"<p class="empty">Keine Treffer.</p>"#)
    );
    w.sender
        .fixed("/json/stations/search", 500, "text/plain", "kaputt");
    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(
        html.contains(r#"<p class="error" role="alert">Senderverzeichnis nicht erreichbar</p>"#)
    );
    // The stations are still there to play.
    assert!(html.contains(r#"data-action="play-station""#));
}

#[sqlx::test]
async fn the_page_adds_by_address_renames_and_removes(db: PgPool) {
    let w = world(db).await;
    w.sender.live("/live", "x");
    w.sender.fixed("/page", 200, "text/html", "<html>");
    let url = w.sender.url("/live");

    // What cannot be added comes back with the reason and the form as it was.
    let bad = w.sender.url("/page");
    let response = form(
        &w,
        "/library/radio",
        &encode(&[("name", "Mein Sender"), ("stream_url", &bad)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_text(response).await;
    assert!(html.contains(&format!(
        r#"<p class="error" role="alert">{NO_AUDIO_TEXT}</p>"#
    )));
    assert!(html.contains(r#"<details class="by-address" open>"#));
    assert!(html.contains(r#"<input name="name" value="Mein Sender""#));
    assert!(html.contains(&format!(r#"<input name="stream_url" value="{bad}""#)));
    let response = form(
        &w,
        "/library/radio",
        &encode(&[("name", ""), ("stream_url", &url)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body_text(response)
            .await
            .contains("Der Name muss 1–100 Zeichen lang sein.")
    );
    assert!(w.names().await.is_empty());

    let response = form(
        &w,
        "/library/radio",
        &encode(&[("name", "Mein Sender"), ("stream_url", &url)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library/radio");
    assert_eq!(w.names().await, ["Mein Sender"]);
    let id: i32 = sqlx::query_scalar("SELECT id FROM radio_stations")
        .fetch_one(w.app.db())
        .await
        .unwrap();

    let rename = format!("/library/radio/{id}/rename");
    let response = form(&w, &rename, &encode(&[("name", " Neuer Name ")])).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(w.names().await, ["Neuer Name"]);
    let response = form(&w, &rename, &encode(&[("name", "  ")])).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(w.names().await, ["Neuer Name"]);

    // Somebody else can do neither.
    let other = w.app.user("bob", false).await;
    let other = format!("autoreverse_session={}", w.app.token(other).await);
    for action in ["rename", "delete"] {
        let uri = format!("/library/radio/{id}/{action}");
        let response = w.app.post_form(&uri, "name=Meins", Some(&other)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{action}");
    }
    assert_eq!(w.names().await, ["Neuer Name"]);

    let response = form(&w, &format!("/library/radio/{id}/delete"), "").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(w.names().await.is_empty());
}

#[sqlx::test]
async fn the_page_needs_a_session(db: PgPool) {
    let w = world(db).await;
    let response = w.app.get("/library/radio", None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/login");
    let response = w
        .app
        .post_form("/library/radio", "name=x&stream_url=y", None)
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[sqlx::test]
async fn without_the_directory_the_page_offers_the_address_form_only(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO_DIRECTORY", "0")]).await;
    let html = page(&w, "/library/radio?q=jazz").await;
    assert!(!html.contains("Sender suchen"));
    assert!(!html.contains("radio-browser.info"));
    assert!(html.contains("Noch keine Sender. Unten die Adresse eines Senders eintragen."));
    assert!(html.contains(r#"<details class="by-address" open>"#));
    assert_eq!(w.sender.hits("/json/stations/search"), 0);
    // A forged form cannot reach the directory either.
    let response = form(&w, "/library/radio", &encode(&[("directory_id", UUID)])).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(w.sender.hits(&format!("/json/stations/byuuid/{UUID}")), 0);
}

#[sqlx::test]
async fn without_the_feature_the_web_ui_has_no_radio(db: PgPool) {
    let w = world_with(db, &[("AUTOREVERSE_RADIO", "0")]).await;
    let cookie = format!("autoreverse_session={}", w.token);
    let response = w.app.get_with_cookie("/library/radio", &cookie).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        body_text(response)
            .await
            .contains("Radio ist auf diesem Server abgeschaltet")
    );
    for (uri, body) in [
        ("/library/radio", "name=x&stream_url=http://x/"),
        ("/library/radio/1/rename", "name=x"),
        ("/library/radio/1/delete", ""),
    ] {
        let response = w.app.post_form(uri, body, Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    // Neither the sidebar nor the Musik chips name it.
    let albums = page(&w, "/library/albums").await;
    assert!(!albums.contains("/library/radio"));
    assert!(albums.contains(r#"<a href="/library/favorites" class="pill">Favoriten</a>"#));
}

// --- SHOUTcast v1 ---------------------------------------------------------------------------

/// A SHOUTcast v1 server: answers every GET with `ICY 200 OK` and an endless MP3 stream, with a
/// title woven in every 16 bytes when asked for one. Returns its address.
async fn shoutcast_v1() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&buffer[..n]),
                    }
                }
                let wants_titles = String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("icy-metadata: 1");
                let mut head =
                    "ICY 200 OK\r\nicy-name:Alt\r\ncontent-type:audio/mpeg\r\nicy-br:128\r\n"
                        .to_owned();
                if wants_titles {
                    head.push_str("icy-metaint:16\r\n");
                }
                head.push_str("\r\n");
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let title = b"StreamTitle='Alte Band - Altes Lied';";
                let blocks = title.len().div_ceil(16);
                let mut meta = vec![blocks as u8];
                meta.extend_from_slice(title);
                meta.resize(1 + blocks * 16, 0);
                loop {
                    let mut piece = vec![0xff, 0xfb];
                    piece.resize(16, b'a');
                    if wants_titles {
                        piece.extend_from_slice(&meta);
                    }
                    if socket.write_all(&piece).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            });
        }
    });
    format!("http://{address}")
}

#[sqlx::test]
async fn an_old_shoutcast_sender_is_added_and_played(db: PgPool) {
    let base = shoutcast_v1().await;
    let library = TempLibrary::new();
    let config = Config::from_lookup(|key| match key {
        "AUTOREVERSE_DATA" => Some(library.data().to_str().unwrap().to_owned()),
        // The fence is lowered for this one local host, as for a mirror in the LAN.
        "AUTOREVERSE_RADIO_DIRECTORY_URL" => Some(base.clone()),
        _ => None,
    })
    .unwrap();
    let app = TestApp::with_state(AppState::new(db, config));
    let user = app.user("ada", false).await;
    let token = app.token(user).await;

    let response = app
        .post_json(
            "/radio/stations",
            json!({"stream_url": format!("{base}/;stream.mp3"), "name": "Alt"}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let station = body_json(response).await;
    assert_eq!(station["codec"], "MP3");
    assert_eq!(station["bitrate"], 128);
    let id = station["id"].as_i64().unwrap();

    let response = app
        .get(&format!("/radio/stations/{id}/stream"), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/mpeg");
    let mut body = response.into_body();
    let audio = read_bytes(&mut body, 200).await;
    // The titles are taken out: what is left is the sender's audio alone.
    assert!(
        audio
            .chunks(16)
            .all(|piece| piece.iter().all(|b| [0xff, 0xfb, b'a'].contains(b))),
        "{audio:?}"
    );
    let now = format!("/radio/stations/{id}/now");
    let mut title = Value::Null;
    for _ in 0..500 {
        title = body_json(app.get(&now, Some(&token)).await).await["title"].clone();
        if !title.is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(title, "Altes Lied");
    drop(body);
}
