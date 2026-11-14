//! Request authentication (legacy `hooks.server.ts` `handle` + `session.ts` guards).
//!
//! One resolver serves API and web: a bearer header first, else the `autoreverse_session` cookie.
//! It runs lazily, the first time a handler asks for [`Session`] or one of the guards, and the
//! result is cached in the request extensions — static files never touch the database.

use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::{HeaderMap, request::Parts};
use axum_extra::extract::cookie::{Cookie, SameSite};

use super::tokens::user_for_token;
use crate::error::{ApiError, WebError};
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "autoreverse_session";

/// Who is calling. `user_id` and `token` are both set or both `None`.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub user_id: Option<i32>,
    pub token: Option<String>,
}

/// The bearer value if the header says `Bearer <token>`, else the cookie. A bearer header that
/// names an invalid token does not fall back to the cookie.
pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let mut parts = authorization.split(' ');
    if let (Some(scheme), Some(value)) = (parts.next(), parts.next())
        && scheme.eq_ignore_ascii_case("bearer")
        && !value.is_empty()
    {
        return Some(value.to_owned());
    }
    session_cookie(headers)
}

pub fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| Cookie::parse(pair.trim().to_owned()).ok())
        .find(|c| c.name() == SESSION_COOKIE)
        .map(|c| c.value().to_owned())
}

/// The session cookie as the web login sets it.
pub fn session_cookie_for(token: String, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .build()
}

/// A `Set-Cookie` value that clears the session cookie.
pub fn cleared_session_cookie(secure: bool) -> Cookie<'static> {
    let mut cookie = session_cookie_for(String::new(), secure);
    cookie.make_removal();
    cookie
}

async fn resolve(parts: &mut Parts, state: &AppState) -> Result<Session, sqlx::Error> {
    if let Some(session) = parts.extensions.get::<Session>() {
        return Ok(session.clone());
    }
    let session = match token_from_headers(&parts.headers) {
        None => Session::default(),
        Some(token) => match user_for_token(&state.db, &token).await? {
            Some(user_id) => Session {
                user_id: Some(user_id),
                token: Some(token),
            },
            None => Session::default(),
        },
    };
    parts.extensions.insert(session.clone());
    Ok(session)
}

impl FromRequestParts<AppState> for Session {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(resolve(parts, state).await?)
    }
}

/// An authenticated API caller; 401 "Kein Token" otherwise.
#[derive(Debug, Clone)]
pub struct ApiUser {
    pub id: i32,
    pub token: String,
}

impl FromRequestParts<AppState> for ApiUser {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match resolve(parts, state).await? {
            Session {
                user_id: Some(id),
                token: Some(token),
            } => Ok(ApiUser { id, token }),
            _ => Err(ApiError::unauthorized("Kein Token")),
        }
    }
}

/// An authenticated admin; 401 without a session, 403 "Nur für Verwalter" without the role.
#[derive(Debug, Clone)]
pub struct ApiAdmin {
    pub id: i32,
    pub token: String,
}

impl FromRequestParts<AppState> for ApiAdmin {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = ApiUser::from_request_parts(parts, state).await?;
        if !is_admin(state, user.id).await? {
            return Err(ApiError::forbidden("Nur für Verwalter"));
        }
        Ok(ApiAdmin {
            id: user.id,
            token: user.token,
        })
    }
}

/// A logged-in web user; redirects to `/login` otherwise.
#[derive(Debug, Clone)]
pub struct WebUser {
    pub id: i32,
    pub token: String,
}

impl FromRequestParts<AppState> for WebUser {
    type Rejection = WebError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match resolve(parts, state).await? {
            Session {
                user_id: Some(id),
                token: Some(token),
            } => Ok(WebUser { id, token }),
            _ => Err(WebError::redirect("/login")),
        }
    }
}

/// A logged-in admin in the web UI; non-admins are sent back to `/library`.
#[derive(Debug, Clone)]
pub struct WebAdmin {
    pub id: i32,
    pub token: String,
}

impl FromRequestParts<AppState> for WebAdmin {
    type Rejection = WebError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = WebUser::from_request_parts(parts, state).await?;
        if !is_admin(state, user.id).await? {
            return Err(WebError::redirect("/library"));
        }
        Ok(WebAdmin {
            id: user.id,
            token: user.token,
        })
    }
}

async fn is_admin(state: &AppState, user_id: i32) -> Result<bool, sqlx::Error> {
    let row: Option<bool> = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(*k, HeaderValue::from_static(v));
        }
        map
    }

    #[test]
    fn bearer_wins_over_the_cookie() {
        let h = headers(&[
            ("authorization", "Bearer abc"),
            ("cookie", "autoreverse_session=def"),
        ]);
        assert_eq!(token_from_headers(&h).as_deref(), Some("abc"));
    }

    #[test]
    fn scheme_is_case_insensitive() {
        let h = headers(&[("authorization", "bearer abc")]);
        assert_eq!(token_from_headers(&h).as_deref(), Some("abc"));
    }

    #[test]
    fn falls_back_to_the_cookie() {
        let h = headers(&[("cookie", "theme=dark; autoreverse_session=def")]);
        assert_eq!(token_from_headers(&h).as_deref(), Some("def"));
        let h = headers(&[
            ("authorization", "Basic xyz"),
            ("cookie", "autoreverse_session=def"),
        ]);
        assert_eq!(token_from_headers(&h).as_deref(), Some("def"));
    }

    #[test]
    fn nothing_means_no_token() {
        assert_eq!(token_from_headers(&HeaderMap::new()), None);
        assert_eq!(
            token_from_headers(&headers(&[("authorization", "Bearer ")])),
            None
        );
    }

    #[test]
    fn cookie_attributes_match_the_legacy_login() {
        let cookie = session_cookie_for("tok".into(), false).to_string();
        assert_eq!(
            cookie,
            "autoreverse_session=tok; HttpOnly; SameSite=Lax; Path=/"
        );
        assert!(
            session_cookie_for("tok".into(), true)
                .to_string()
                .contains("Secure")
        );
        assert!(
            cleared_session_cookie(false)
                .to_string()
                .contains("Max-Age=0")
        );
    }
}
