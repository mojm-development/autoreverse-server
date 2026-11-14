//! Remote control between the devices of an account (`/me/devices…`): the event stream, states,
//! commands, handing over, the limits and the switch.

use std::sync::Arc;
use std::time::Duration;

use autoreverse_server::devices::{Hub, Timing};
use autoreverse_server::{AppState, Config};
use axum::body::Body;
use axum::http::{Method, Response, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{TestApp, body_json, header_str};

const A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const B: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const GRACE: Duration = Duration::from_millis(400);

/// The app with waits short enough to test: the real ones are 20 s, 15 s and 30 s.
fn fast(db: PgPool) -> TestApp {
    let mut state = AppState::new(db, Config::from_lookup(|_| None).unwrap());
    state.devices = Arc::new(Hub::new(Timing {
        grace: GRACE,
        ping: Duration::from_millis(100),
        recheck: Duration::from_millis(100),
    }));
    TestApp::with_state(state)
}

async fn login(app: &TestApp, name: &str) -> String {
    let user = app.user(name, false).await;
    app.token(user).await
}

/// The open event stream of a device, read event by event.
struct Events {
    body: Body,
    buffer: String,
}

impl Events {
    /// The next block of the stream (an event or a comment), `None` at its end. Panics when
    /// nothing comes for two seconds.
    async fn block(&mut self) -> Option<String> {
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block = self.buffer[..end].to_owned();
                self.buffer.drain(..end + 2);
                return Some(block);
            }
            let frame = tokio::time::timeout(Duration::from_secs(2), self.body.frame())
                .await
                .expect("the stream stayed silent");
            match frame {
                Some(frame) => {
                    let data = frame.unwrap().into_data().unwrap();
                    self.buffer.push_str(std::str::from_utf8(&data).unwrap());
                }
                None => return None,
            }
        }
    }

    /// The next event as `(name, data)`, pings skipped; `None` at the end of the stream.
    async fn next(&mut self) -> Option<(String, Value)> {
        loop {
            let block = self.block().await?;
            if block == ": ping" {
                continue;
            }
            let (event, data) = block.split_once('\n').expect("event and data line");
            let event = event.strip_prefix("event: ").expect("an event line");
            let data = data.strip_prefix("data: ").expect("a single data line");
            return Some((event.to_owned(), serde_json::from_str(data).unwrap()));
        }
    }

    async fn expect(&mut self, event: &str) -> Value {
        let (name, data) = self.next().await.expect("the stream ended");
        assert_eq!(name, event, "{data}");
        data
    }

    /// Asserts that no event (pings aside) arrives within `wait`.
    async fn silent_for(&mut self, wait: Duration) {
        if let Ok(event) = tokio::time::timeout(wait, self.next()).await {
            panic!("unexpected event: {event:?}");
        }
    }
}

async fn open_response(
    app: &TestApp,
    token: &str,
    id: &str,
    name: &str,
    kind: &str,
) -> Response<Body> {
    let query =
        serde_urlencoded::to_string([("device_id", id), ("name", name), ("kind", kind)]).unwrap();
    app.get(&format!("/me/devices/events?{query}"), Some(token))
        .await
}

/// Connects a device and reads the `devices` event every stream starts with.
async fn connect(app: &TestApp, token: &str, id: &str, name: &str) -> (Events, Value) {
    let response = open_response(app, token, id, name, "phone").await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut events = Events {
        body: response.into_body(),
        buffer: String::new(),
    };
    let first = events.expect("devices").await;
    (events, first)
}

fn track_state(playing: bool, position: f64, at: &str) -> Value {
    json!({
        "playing": playing,
        "media": { "kind": "track", "item_id": 7, "track_id": 70, "station_id": null,
                   "title": "Human Behaviour", "artist": "Björk", "item_title": "Debut" },
        "position": position, "duration": 252.0, "volume": 0.8, "shuffle": false, "repeat": "off",
        "queue": [{ "item_id": 7, "track_id": 70 }, { "item_id": 7, "track_id": 71 }],
        "context": "Debut", "at": at
    })
}

async fn put_state(app: &TestApp, token: &str, id: &str, state: Value) -> StatusCode {
    app.call(
        Method::PUT,
        &format!("/me/devices/{id}/state"),
        Some(state),
        Some(token),
    )
    .await
    .status()
}

async fn send_command(app: &TestApp, token: &str, id: &str, body: Value) -> Response<Body> {
    app.post_json(&format!("/me/devices/{id}/commands"), body, Some(token))
        .await
}

async fn listed(app: &TestApp, token: &str) -> Value {
    body_json(app.get("/me/devices", Some(token)).await).await
}

fn ids(list: &Value) -> Vec<&str> {
    list["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|device| device["id"].as_str().unwrap())
        .collect()
}

#[sqlx::test]
async fn the_stream_starts_with_the_devices_and_carries_the_proxy_headers(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let response = open_response(&app, &token, A, "Oskars Telefon", "phone").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_str(&response, header::CONTENT_TYPE),
        "text/event-stream"
    );
    assert_eq!(header_str(&response, header::CACHE_CONTROL), "no-cache");
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    let mut events = Events {
        body: response.into_body(),
        buffer: String::new(),
    };
    let expected = json!({ "devices": [{
        "id": A, "name": "Oskars Telefon", "kind": "phone", "online": true, "active": false,
        "state": null
    }] });
    assert_eq!(events.expect("devices").await, expected);
    assert_eq!(listed(&app, &token).await, expected);
}

#[sqlx::test]
async fn a_ping_comment_keeps_the_stream_alive(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut events, _) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(events.block().await.as_deref(), Some(": ping"));
    assert_eq!(events.block().await.as_deref(), Some(": ping"));
}

#[sqlx::test]
async fn the_stream_needs_a_login_and_sound_parameters(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let response = app
        .get(
            &format!("/me/devices/events?device_id={A}&name=x&kind=phone"),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for (id, name, kind, detail) in [
        ("short", "Telefon", "phone", "Ungültige Gerätekennung"),
        (
            "has_underscore_in_it",
            "Telefon",
            "phone",
            "Ungültige Gerätekennung",
        ),
        (A, "   ", "phone", "name muss 1 bis 60 Zeichen lang sein"),
        (
            A,
            &"x".repeat(61),
            "phone",
            "name muss 1 bis 60 Zeichen lang sein",
        ),
        (
            A,
            "Telefon",
            "toaster",
            "kind muss desktop, phone, tablet, web oder car sein",
        ),
    ] {
        let response = open_response(&app, &token, id, name, kind).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{id} {kind}"
        );
        assert_eq!(body_json(response).await["detail"], detail);
    }
    let response = app.get("/me/devices/events", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(listed(&app, &token).await, json!({ "devices": [] }));
    for kind in ["desktop", "phone", "tablet", "web", "car"] {
        assert_eq!(
            open_response(&app, &token, A, "Gerät", kind).await.status(),
            StatusCode::OK
        );
    }
}

#[sqlx::test]
async fn a_second_connection_of_a_device_replaces_the_first(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut watcher, _) = connect(&app, &token, B, "Rechner").await;
    let (mut first, _) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(ids(&watcher.expect("devices").await), [B, A]);

    let (mut second, list) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(ids(&list), [B, A]);
    // The old stream is ended by the server …
    assert_eq!(first.next().await, None);
    // … which is not the device going away: it stays listed beyond the grace period, and the
    // others were never told anything.
    tokio::time::sleep(GRACE + Duration::from_millis(200)).await;
    assert_eq!(ids(&listed(&app, &token).await), [B, A]);
    watcher.silent_for(Duration::from_millis(50)).await;
    // The new stream is the one that gets the device's commands.
    let sent = send_command(&app, &token, A, json!({ "command": "pause", "from": B })).await;
    assert_eq!(sent.status(), StatusCode::ACCEPTED);
    assert_eq!(second.expect("command").await["command"], "pause");
}

#[sqlx::test]
async fn a_lost_stream_counts_as_online_for_the_grace_period(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut watcher, _) = connect(&app, &token, B, "Rechner").await;
    let (phone, _) = connect(&app, &token, A, "Telefon").await;
    watcher.expect("devices").await;

    // The connection breaks and comes back in time: nobody notices.
    drop(phone);
    assert_eq!(ids(&listed(&app, &token).await), [B, A]);
    tokio::time::sleep(GRACE / 2).await;
    let (phone, list) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(ids(&list), [B, A]);
    tokio::time::sleep(GRACE + Duration::from_millis(200)).await;
    assert_eq!(ids(&listed(&app, &token).await), [B, A]);
    watcher.silent_for(Duration::from_millis(50)).await;

    // It breaks for good: after the grace period the device is offline and the others are told.
    drop(phone);
    watcher.silent_for(GRACE / 2).await;
    assert_eq!(ids(&watcher.expect("devices").await), [B]);
    assert_eq!(ids(&listed(&app, &token).await), [B]);
}

#[sqlx::test]
async fn a_new_name_is_passed_on(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut watcher, _) = connect(&app, &token, B, "Rechner").await;
    let (_phone, _) = connect(&app, &token, A, "Telefon").await;
    watcher.expect("devices").await;
    let (_renamed, _) = connect(&app, &token, A, "Oskars Telefon").await;
    let list = watcher.expect("devices").await;
    assert_eq!(list["devices"][0]["name"], "Oskars Telefon");
}

#[sqlx::test]
async fn a_state_is_passed_on_but_mere_progress_is_not(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut watcher, _) = connect(&app, &token, B, "Rechner").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    watcher.expect("devices").await;
    phone.silent_for(Duration::from_millis(30)).await;

    let state = track_state(true, 10.0, "2026-11-13T12:00:00+00:00");
    assert_eq!(
        put_state(&app, &token, A, state).await,
        StatusCode::NO_CONTENT
    );
    let list = watcher.expect("devices").await;
    let device = &list["devices"][1];
    assert_eq!(device["id"], A);
    assert_eq!(device["active"], true);
    // Complete, and `at` in the one format every client reads.
    assert_eq!(
        device["state"],
        track_state(true, 10.0, "2026-11-13T12:00:00.000Z")
    );
    assert_eq!(list["devices"][0]["active"], false);
    // The reporting device hears it too: it is how it learns that it is the active one.
    assert_eq!(phone.expect("devices").await, list);

    // Ten seconds on, ten seconds further: stored, but nobody is told.
    let later = track_state(true, 20.3, "2026-11-13T12:00:10Z");
    assert_eq!(
        put_state(&app, &token, A, later).await,
        StatusCode::NO_CONTENT
    );
    let again = track_state(true, 30.1, "2026-11-13T12:00:20Z");
    assert_eq!(
        put_state(&app, &token, A, again).await,
        StatusCode::NO_CONTENT
    );
    watcher.silent_for(Duration::from_millis(150)).await;
    assert_eq!(
        listed(&app, &token).await["devices"][1]["state"]["position"],
        30.1
    );

    // A pause is told …
    let paused = track_state(false, 31.0, "2026-11-13T12:00:21Z");
    assert_eq!(
        put_state(&app, &token, A, paused).await,
        StatusCode::NO_CONTENT
    );
    let list = watcher.expect("devices").await;
    assert_eq!(list["devices"][1]["state"]["playing"], false);
    // … the paused device stays the active one …
    assert_eq!(list["devices"][1]["active"], true);
    // … and so are a jump and another track.
    let jumped = track_state(false, 120.0, "2026-11-13T12:00:22Z");
    assert_eq!(
        put_state(&app, &token, A, jumped).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        watcher.expect("devices").await["devices"][1]["state"]["position"],
        120.0
    );
    let mut next = track_state(false, 0.0, "2026-11-13T12:00:23Z");
    next["media"]["track_id"] = json!(71);
    assert_eq!(
        put_state(&app, &token, A, next).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        watcher.expect("devices").await["devices"][1]["state"]["media"]["track_id"],
        71
    );
}

#[sqlx::test]
async fn the_last_device_to_play_is_the_active_one(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut a, _) = connect(&app, &token, A, "Telefon").await;
    let (_b, _) = connect(&app, &token, B, "Rechner").await;
    a.expect("devices").await;
    let now = "2026-11-13T12:00:00Z";
    put_state(&app, &token, A, track_state(true, 1.0, now)).await;
    put_state(&app, &token, B, track_state(true, 1.0, now)).await;
    a.expect("devices").await;
    let list = a.expect("devices").await;
    assert_eq!(list["devices"][0]["active"], true, "{list}");
    assert_eq!(list["devices"][1]["active"], false);
    assert_eq!(list["devices"][0]["id"], B);

    // The active device goes offline: nobody is active.
    app.call(
        Method::DELETE,
        &format!("/me/devices/{B}"),
        None,
        Some(&token),
    )
    .await;
    let list = a.expect("devices").await;
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    assert_eq!(list["devices"][0]["active"], false);
}

#[sqlx::test]
async fn a_state_is_checked(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (_phone, _) = connect(&app, &token, A, "Telefon").await;
    let at = "2026-11-13T12:00:00Z";
    let with = |key: &str, value: Value| {
        let mut state = track_state(true, 1.0, at);
        state[key] = value;
        state
    };
    for (bad, detail) in [
        (
            with("position", json!(-1)),
            "position außerhalb des gültigen Bereichs",
        ),
        (
            with("duration", json!(-5)),
            "duration außerhalb des gültigen Bereichs",
        ),
        (
            with("volume", json!(1.2)),
            "volume außerhalb des gültigen Bereichs",
        ),
        (
            with(
                "queue",
                json!(vec![json!({ "item_id": 1, "track_id": 2 }); 301]),
            ),
            "queue ist zu lang (höchstens 300 Einträge)",
        ),
        (with("repeat", json!("immer")), "Ungültiger JSON-Body"),
        (with("playing", json!("ja")), "Ungültiger JSON-Body"),
        (with("at", json!("gerade eben")), "Ungültiger JSON-Body"),
        (
            with("media", json!({ "kind": "film", "title": "x" })),
            "Ungültiger JSON-Body",
        ),
        (json!({ "playing": true }), "Ungültiger JSON-Body"),
    ] {
        let response = app
            .call(
                Method::PUT,
                &format!("/me/devices/{A}/state"),
                Some(bad),
                Some(&token),
            )
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{detail}"
        );
        assert_eq!(body_json(response).await["detail"], detail);
    }
    assert!(listed(&app, &token).await["devices"][0]["state"].is_null());

    // The smallest state there is, and one of each kind of medium.
    let minimal = json!({ "playing": false, "position": 0, "at": at });
    assert_eq!(
        put_state(&app, &token, A, minimal).await,
        StatusCode::NO_CONTENT
    );
    for kind in ["album", "book", "episode", "track", "station"] {
        let mut state = track_state(true, 1.0, at);
        state["media"]["kind"] = json!(kind);
        assert_eq!(
            put_state(&app, &token, A, state).await,
            StatusCode::NO_CONTENT,
            "{kind}"
        );
    }
    let full = with(
        "queue",
        json!(vec![json!({ "item_id": 1, "track_id": null }); 300]),
    );
    assert_eq!(
        put_state(&app, &token, A, full).await,
        StatusCode::NO_CONTENT
    );

    // A device that never opened a stream has no name to be listed under.
    let response = app
        .call(
            Method::PUT,
            &format!("/me/devices/{B}/state"),
            Some(track_state(true, 1.0, at)),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["detail"],
        "Das Gerät ist nicht verbunden"
    );
    assert_eq!(
        put_state(&app, &token, "x", track_state(true, 1.0, at)).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test]
async fn commands_reach_the_target_device(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    for (command, value) in [
        ("play", Value::Null),
        ("pause", Value::Null),
        ("toggle", Value::Null),
        ("next", Value::Null),
        ("previous", Value::Null),
        ("stop", Value::Null),
        ("seek", json!(93.5)),
        ("volume", json!(0.25)),
        ("shuffle", json!(true)),
        ("repeat", json!("all")),
    ] {
        // The sender needs no stream of its own.
        let body = json!({ "command": command, "value": value, "from": B });
        let response = send_command(&app, &token, A, body.clone()).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{command}");
        assert_eq!(phone.expect("command").await, body);
    }
    // `value` may be left out where the command takes none.
    let response = send_command(&app, &token, A, json!({ "command": "next", "from": B })).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        phone.expect("command").await,
        json!({ "command": "next", "value": null, "from": B })
    );
}

#[sqlx::test]
async fn bad_commands_are_refused(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    for (body, detail) in [
        (
            json!({ "command": "dance", "from": B }),
            "Unbekannter Befehl",
        ),
        (
            json!({ "command": "seek", "from": B }),
            "seek braucht als value eine Zahl ab 0 (Sekunden)",
        ),
        (
            json!({ "command": "seek", "value": -3, "from": B }),
            "seek braucht als value eine Zahl ab 0 (Sekunden)",
        ),
        (
            json!({ "command": "volume", "value": 2, "from": B }),
            "volume braucht als value eine Zahl von 0 bis 1",
        ),
        (
            json!({ "command": "shuffle", "value": "an", "from": B }),
            "shuffle braucht als value true oder false",
        ),
        (
            json!({ "command": "repeat", "value": true, "from": B }),
            "repeat braucht als value \"off\", \"all\" oder \"one\"",
        ),
        (
            json!({ "command": "play", "from": "x" }),
            "from ist keine gültige Gerätekennung",
        ),
        (json!({ "command": "play" }), "Ungültiger JSON-Body"),
        (json!({ "from": B }), "Ungültiger JSON-Body"),
    ] {
        let response = send_command(&app, &token, A, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{detail}"
        );
        assert_eq!(body_json(response).await["detail"], detail);
    }
    let response = app
        .post_raw(&format!("/me/devices/{A}/commands"), "{", Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    phone.silent_for(Duration::from_millis(50)).await;
}

#[sqlx::test]
async fn a_command_to_an_offline_or_unknown_device_is_a_conflict(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let play = json!({ "command": "play", "from": B });
    let unknown = send_command(&app, &token, A, play.clone()).await;
    assert_eq!(unknown.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(unknown).await["detail"],
        "Das Gerät ist nicht verbunden"
    );

    let (phone, _) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(
        send_command(&app, &token, A, play.clone()).await.status(),
        StatusCode::ACCEPTED
    );
    // Within the grace period the device is still listed, but there is no stream to send to.
    drop(phone);
    assert_eq!(
        send_command(&app, &token, A, play.clone()).await.status(),
        StatusCode::CONFLICT
    );
    tokio::time::sleep(GRACE + Duration::from_millis(200)).await;
    assert_eq!(
        send_command(&app, &token, A, play).await.status(),
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn two_accounts_neither_see_nor_reach_each_other(db: PgPool) {
    let app = fast(db);
    let oskar = login(&app, "oskar").await;
    let mara = login(&app, "mara").await;
    let (mut oskars, first) = connect(&app, &oskar, A, "Oskars Telefon").await;
    assert_eq!(ids(&first), [A]);
    // Even the same device id is another device in another account.
    let (mut maras, first) = connect(&app, &mara, A, "Maras Telefon").await;
    assert_eq!(first["devices"][0]["name"], "Maras Telefon");
    assert_eq!(ids(&first), [A]);
    let (_maras_other, _) = connect(&app, &mara, B, "Maras Rechner").await;
    maras.expect("devices").await;

    let at = "2026-11-13T12:00:00Z";
    assert_eq!(
        put_state(&app, &mara, A, track_state(true, 5.0, at)).await,
        StatusCode::NO_CONTENT
    );
    maras.expect("devices").await;
    oskars.silent_for(Duration::from_millis(100)).await;
    let list = listed(&app, &oskar).await;
    assert_eq!(ids(&list), [A]);
    assert!(list["devices"][0]["state"].is_null());

    // Oskar cannot reach Mara's second device: for him it does not exist.
    let play = json!({ "command": "play", "from": A });
    assert_eq!(
        send_command(&app, &oskar, B, play).await.status(),
        StatusCode::CONFLICT
    );
    let response = app
        .post_json(
            &format!("/me/devices/{B}/transfer"),
            json!({ "from": A }),
            Some(&oskar),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        put_state(&app, &oskar, B, track_state(true, 5.0, at)).await,
        StatusCode::CONFLICT
    );
    // A command to "his" A reaches his device, not Mara's of the same id.
    let pause = json!({ "command": "pause", "from": B });
    assert_eq!(
        send_command(&app, &oskar, A, pause).await.status(),
        StatusCode::ACCEPTED
    );
    oskars.expect("command").await;
    maras.silent_for(Duration::from_millis(100)).await;
    // Nor can he sign her device off.
    app.call(
        Method::DELETE,
        &format!("/me/devices/{B}"),
        None,
        Some(&oskar),
    )
    .await;
    assert_eq!(ids(&listed(&app, &mara).await), [B, A]);
}

async fn post_transfer(app: &TestApp, token: &str, target: &str, body: Value) -> Response<Body> {
    app.post_json(&format!("/me/devices/{target}/transfer"), body, Some(token))
        .await
}

#[sqlx::test]
async fn a_transfer_hands_the_moved_on_state_to_the_target_and_stops_the_source(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    let (mut desk, _) = connect(&app, &token, B, "Rechner").await;
    phone.expect("devices").await;
    // A clock that is years off: handing over goes by when the report came in.
    put_state(
        &app,
        &token,
        A,
        track_state(true, 10.0, "2020-01-01T00:00:00Z"),
    )
    .await;
    phone.expect("devices").await;
    desk.expect("devices").await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = chrono::Utc::now();
    let response = post_transfer(&app, &token, B, json!({ "from": A })).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let take_over = desk.expect("take_over").await;
    assert_eq!(take_over["from"], A);
    assert_eq!(take_over["play"], true);
    let state = &take_over["state"];
    let position = state["position"].as_f64().unwrap();
    assert!((10.3..11.5).contains(&position), "{position}");
    let at: chrono::DateTime<chrono::Utc> = state["at"].as_str().unwrap().parse().unwrap();
    assert!((at - before).num_seconds().abs() < 2, "{at}");
    assert!(state["at"].as_str().unwrap().ends_with('Z'));
    // Everything else is what the source reported.
    let mut expected = track_state(true, 0.0, "");
    for moved in ["position", "at"] {
        expected[moved] = state[moved].clone();
    }
    assert_eq!(*state, expected);

    assert_eq!(
        phone.expect("command").await,
        json!({ "command": "stop", "value": null, "from": B })
    );
    // Being asked is not yet playing: the source stays the active one …
    let list = listed(&app, &token).await;
    assert_eq!(list["devices"][0]["active"], false);
    assert_eq!(list["devices"][1]["active"], true);
    // … until the target reports — even paused, it is where playback now is.
    put_state(
        &app,
        &token,
        A,
        json!({ "playing": false, "position": 0, "at": "2020-01-01T00:00:01Z" }),
    )
    .await;
    put_state(
        &app,
        &token,
        B,
        track_state(false, position, "2026-11-13T12:00:00Z"),
    )
    .await;
    let list = listed(&app, &token).await;
    assert_eq!(list["devices"][0]["id"], B);
    assert_eq!(list["devices"][0]["active"], true);
    assert_eq!(list["devices"][1]["active"], false);
}

#[sqlx::test]
async fn a_transfer_without_a_source_takes_the_active_device(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    let (mut desk, _) = connect(&app, &token, B, "Rechner").await;
    phone.expect("devices").await;

    // Nobody plays: nothing to take.
    let nothing = post_transfer(&app, &token, B, json!({ "from": null })).await;
    assert_eq!(nothing.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(nothing).await["detail"],
        "Es läuft nichts, das übernommen werden könnte"
    );
    // Nor from a device that has not said what it plays, or says that it plays nothing.
    assert_eq!(
        post_transfer(&app, &token, B, json!({ "from": A }))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    put_state(
        &app,
        &token,
        A,
        json!({ "playing": false, "position": 0, "at": "2026-11-13T12:00:00Z" }),
    )
    .await;
    assert_eq!(
        post_transfer(&app, &token, B, json!({ "from": A }))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    phone.expect("devices").await;
    desk.expect("devices").await;

    // A paused source hands over where it stands, and `play: false` is passed on.
    put_state(
        &app,
        &token,
        A,
        track_state(true, 40.0, "2026-11-13T12:00:00Z"),
    )
    .await;
    put_state(
        &app,
        &token,
        A,
        track_state(false, 42.0, "2026-11-13T12:00:02Z"),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let response = post_transfer(&app, &token, B, json!({ "from": null, "play": false })).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    desk.expect("devices").await;
    desk.expect("devices").await;
    let take_over = desk.expect("take_over").await;
    assert_eq!(take_over["from"], A);
    assert_eq!(take_over["play"], false);
    assert_eq!(take_over["state"]["position"], 42.0);

    // The active device cannot take over from itself.
    let own = post_transfer(&app, &token, A, json!({ "from": null })).await;
    assert_eq!(own.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(own).await["detail"],
        "Die Wiedergabe läuft bereits auf diesem Gerät"
    );
    // A target that is not there, and bodies that are not right.
    let gone = "cccccccc-0000-4000-8000-000000000003";
    let offline = post_transfer(&app, &token, gone, json!({ "from": A })).await;
    assert_eq!(offline.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(offline).await["detail"],
        "Das Gerät ist nicht verbunden"
    );
    for bad in [json!({ "from": "x" }), json!({ "from": A, "play": "ja" })] {
        let response = post_transfer(&app, &token, B, bad).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[sqlx::test]
async fn a_live_station_is_handed_over_without_a_position(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (_phone, _) = connect(&app, &token, A, "Telefon").await;
    let (mut desk, _) = connect(&app, &token, B, "Rechner").await;
    let station = json!({
        "playing": true, "position": 0, "duration": 0, "at": "2026-11-13T12:00:00Z",
        "media": { "kind": "station", "station_id": 3, "title": "Radio Eins" }
    });
    put_state(&app, &token, A, station).await;
    desk.expect("devices").await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    post_transfer(&app, &token, B, json!({ "from": null })).await;
    let take_over = desk.expect("take_over").await;
    assert_eq!(take_over["state"]["position"], 0.0);
    assert_eq!(take_over["state"]["media"]["station_id"], 3);
}

#[sqlx::test]
async fn signing_off_ends_the_stream_at_once(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut watcher, _) = connect(&app, &token, B, "Rechner").await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    watcher.expect("devices").await;
    let response = app
        .call(
            Method::DELETE,
            &format!("/me/devices/{A}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // No grace period for a device that says goodbye.
    assert_eq!(ids(&watcher.expect("devices").await), [B]);
    assert_eq!(phone.next().await, None);
    // Unknown devices sign off just as well.
    let again = app
        .call(
            Method::DELETE,
            &format!("/me/devices/{A}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(again.status(), StatusCode::NO_CONTENT);
    watcher.silent_for(Duration::from_millis(50)).await;
}

#[sqlx::test]
async fn the_twenty_first_device_pushes_out_the_one_silent_longest(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let id = |n: usize| format!("device-{n:04}");
    let mut streams = Vec::new();
    for n in 0..20 {
        streams.push(
            connect(&app, &token, &id(n), &format!("Gerät {n:02}"))
                .await
                .0,
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    // The first device speaks up, so the second is the one that was silent longest.
    put_state(
        &app,
        &token,
        &id(0),
        track_state(true, 1.0, "2026-11-13T12:00:00Z"),
    )
    .await;
    let (_newcomer, list) = connect(&app, &token, &id(20), "Gerät 20").await;
    let listed = ids(&list);
    assert_eq!(listed.len(), 20);
    assert!(!listed.contains(&id(1).as_str()));
    assert!(listed.contains(&id(0).as_str()) && listed.contains(&id(20).as_str()));
    // Its stream is ended.
    let pushed_out = &mut streams[1];
    while pushed_out.next().await.is_some() {}
}

#[sqlx::test]
async fn commands_to_one_device_are_throttled(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (_phone, _) = connect(&app, &token, A, "Telefon").await;
    let (_desk, _) = connect(&app, &token, B, "Rechner").await;
    let volume = json!({ "command": "volume", "value": 0.5, "from": B });
    for _ in 0..20 {
        assert_eq!(
            send_command(&app, &token, A, volume.clone()).await.status(),
            StatusCode::ACCEPTED
        );
    }
    let refused = send_command(&app, &token, A, volume.clone()).await;
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(header_str(&refused, header::RETRY_AFTER), "1");
    assert_eq!(
        body_json(refused).await["detail"],
        "Zu viele Befehle an dieses Gerät"
    );
    // The limit is per target, and a second later it is over.
    let other = json!({ "command": "volume", "value": 0.5, "from": A });
    assert_eq!(
        send_command(&app, &token, B, other).await.status(),
        StatusCode::ACCEPTED
    );
    tokio::time::sleep(Duration::from_millis(1050)).await;
    assert_eq!(
        send_command(&app, &token, A, volume).await.status(),
        StatusCode::ACCEPTED
    );
}

#[sqlx::test]
async fn a_receiver_that_does_not_read_loses_its_stream_and_blocks_nobody(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let (mut sleeper, _) = connect(&app, &token, A, "Telefon").await;
    let (mut desk, _) = connect(&app, &token, B, "Rechner").await;
    // Every one of these is worth an event; the sleeper reads none of them.
    for n in 0..100 {
        let state = track_state(n % 2 == 0, 1.0, "2026-11-13T12:00:00Z");
        assert_eq!(
            put_state(&app, &token, B, state).await,
            StatusCode::NO_CONTENT
        );
        desk.expect("devices").await;
    }
    // What fitted into its buffer is delivered, then the stream ends …
    let mut delivered = 0;
    while sleeper.next().await.is_some() {
        delivered += 1;
    }
    assert!((1..100).contains(&delivered), "{delivered}");
    // … and the device reconnects as after any other break.
    assert_eq!(ids(&listed(&app, &token).await), [B, A]);
    let (_awake, list) = connect(&app, &token, A, "Telefon").await;
    assert_eq!(ids(&list), [B, A]);
}

#[sqlx::test]
async fn a_logout_ends_the_streams_of_its_token(db: PgPool) {
    let app = fast(db);
    let user = app.user("oskar", false).await;
    let phone_token = app.token(user).await;
    let desk_token = app.token(user).await;
    let (mut phone, _) = connect(&app, &phone_token, A, "Telefon").await;
    let (mut desk, _) = connect(&app, &desk_token, B, "Rechner").await;
    phone.expect("devices").await;

    let response = app.post_raw("/auth/logout", "", Some(&phone_token)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(phone.next().await, None);
    // The other session of the account is untouched and sees the device go after the grace.
    assert_eq!(ids(&desk.expect("devices").await), [B]);
}

#[sqlx::test]
async fn a_token_revoked_elsewhere_ends_its_stream_soon(db: PgPool) {
    let app = fast(db);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    let (mut phone, _) = connect(&app, &token, A, "Telefon").await;
    // What a password change does to the other sessions.
    sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1")
        .bind(user)
        .execute(app.db())
        .await
        .unwrap();
    assert_eq!(phone.next().await, None);
}

#[sqlx::test]
async fn the_switch_takes_the_routes_away(db: PgPool) {
    let config =
        Config::from_lookup(|key| (key == "AUTOREVERSE_DEVICES").then(|| "0".to_owned())).unwrap();
    let app = TestApp::with_config(db, config);
    let token = login(&app, "oskar").await;
    let state = track_state(true, 1.0, "2026-11-13T12:00:00Z");
    for (method, path, body) in [
        (Method::GET, "/me/devices".to_owned(), None),
        (
            Method::GET,
            format!("/me/devices/events?device_id={A}&name=x&kind=web"),
            None,
        ),
        (Method::PUT, format!("/me/devices/{A}/state"), Some(state)),
        (
            Method::POST,
            format!("/me/devices/{A}/commands"),
            Some(json!({ "command": "play", "from": B })),
        ),
        (
            Method::POST,
            format!("/me/devices/{A}/transfer"),
            Some(json!({ "from": null })),
        ),
        (Method::DELETE, format!("/me/devices/{A}"), None),
    ] {
        for token in [Some(token.as_str()), None] {
            let response = app.call(method.clone(), &path, body.clone(), token).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
            assert_eq!(
                body_json(response).await["detail"],
                "Die Fernsteuerung ist auf diesem Server abgeschaltet"
            );
        }
    }
    let info = body_json(app.get("/server/info", Some(&token)).await).await;
    assert_eq!(info["devices"], false);
}

#[sqlx::test]
async fn server_info_names_the_feature(db: PgPool) {
    let app = TestApp::new(db);
    let token = login(&app, "oskar").await;
    let info = body_json(app.get("/server/info", Some(&token)).await).await;
    assert_eq!(info["devices"], true);
}

#[sqlx::test]
async fn the_browser_reaches_the_stream_with_its_session_cookie(db: PgPool) {
    let app = fast(db);
    let token = login(&app, "oskar").await;
    let response = app
        .get_with_cookie(
            &format!("/me/devices/events?device_id={A}&name=Browser%20auf%20Linux&kind=web"),
            &format!("autoreverse_session={token}"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut events = Events {
        body: response.into_body(),
        buffer: String::new(),
    };
    let list = events.expect("devices").await;
    assert_eq!(list["devices"][0]["name"], "Browser auf Linux");
    assert_eq!(list["devices"][0]["kind"], "web");
}

#[sqlx::test]
async fn the_web_player_shows_the_devices_only_where_the_server_offers_them(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = login(&app, "oskar").await;
    let cookie = format!("autoreverse_session={token}");
    let html = crate::common::body_text(app.get_with_cookie("/library", &cookie).await).await;
    assert!(html.contains(r#"data-mini-player hidden data-devices="true""#));
    assert!(html.contains("data-devices-panel"));
    assert!(html.contains(r#"data-action="devices""#));
    assert!(html.contains("Hier weiterhören"));

    let config =
        Config::from_lookup(|key| (key == "AUTOREVERSE_DEVICES").then(|| "0".to_owned())).unwrap();
    let off = TestApp::with_config(db, config);
    let html = crate::common::body_text(off.get_with_cookie("/library", &cookie).await).await;
    assert!(html.contains(r#"data-mini-player hidden data-devices="false""#));
    assert!(!html.contains("data-devices-panel"));
    assert!(!html.contains(r#"data-action="devices""#));
}

/// Over a real socket, as a client sees it: the events arrive as they are sent (nothing waits
/// for a buffer to fill), and a connection that is cut is noticed.
#[sqlx::test]
async fn over_a_socket_the_events_flow_and_a_cut_connection_is_noticed(db: PgPool) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let app = fast(db);
    let token = login(&app, "oskar").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = autoreverse_server::app(app.state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /me/devices/events?device_id={A}&name=Telefon&kind=phone HTTP/1.1\r\n\
         Host: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut received = String::new();
    let mut chunk = [0u8; 4096];
    // The headers, the first event and a ping, each as soon as it is written.
    while !(received.contains("event: devices") && received.contains(": ping")) {
        let read = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut chunk))
            .await
            .expect("the stream stayed silent")
            .unwrap();
        assert!(read > 0, "the server closed the stream: {received}");
        received.push_str(std::str::from_utf8(&chunk[..read]).unwrap());
    }
    let head = received.to_ascii_lowercase();
    assert!(head.starts_with("http/1.1 200"));
    assert!(head.contains("content-type: text/event-stream"));
    assert!(head.contains("transfer-encoding: chunked"));
    assert_eq!(ids(&listed(&app, &token).await), [A]);

    drop(socket);
    // Noticed with the next line the server tries to write, then the grace period.
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if ids(&listed(&app, &token).await).is_empty() {
            return;
        }
    }
    panic!("the device stayed online after its connection was cut");
}
