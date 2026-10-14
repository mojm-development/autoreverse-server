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
