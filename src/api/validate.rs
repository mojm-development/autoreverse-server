//! Request parsing that turns malformed input into 422 with a German `detail` (legacy
//! `validate.ts`), never into a 500 or axum's English plain-text rejection.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::ApiError;

pub const INVALID_JSON: &str = "Ungültiger JSON-Body";
pub const INVALID_QUERY: &str = "Ungültige Query-Parameter";

/// A JSON body. Like `request.json()` in legacy it ignores the content type; anything that does
/// not parse into `T` is a 422 "Ungültiger JSON-Body".
#[derive(Debug, Clone)]
pub struct ApiJson<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|_| ApiError::unprocessable(INVALID_JSON))?;
        serde_json::from_slice(&bytes)
            .map(ApiJson)
            .map_err(|_| ApiError::unprocessable(INVALID_JSON))
    }
}

/// Typed query parameters; a parameter that does not fit `T` is a 422.
#[derive(Debug, Clone)]
pub struct ApiQuery<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or("");
        serde_urlencoded::from_str(query)
            .map(ApiQuery)
            .map_err(|_| ApiError::unprocessable(INVALID_QUERY))
    }
}

/// Raw query parameters, for routes that validate one by one with [`int_param`].
pub type QueryMap = HashMap<String, String>;

/// An integer query parameter within `[min, max]`, `def` when absent (legacy `intParam`).
pub fn int_param(
    query: &QueryMap,
    name: &str,
    def: i64,
    min: i64,
    max: i64,
) -> Result<i64, ApiError> {
    let Some(raw) = query.get(name) else {
        return Ok(def);
    };
    let out_of_range =
        || ApiError::unprocessable(format!("{name} außerhalb des gültigen Bereichs"));
    // `Number(raw)` accepted "1e2" and " 7 "; whole numbers are what matter here.
    let n: f64 = raw.trim().parse().map_err(|_| out_of_range())?;
    if !n.is_finite() || n.fract() != 0.0 || n < min as f64 || n > max as f64 {
        return Err(out_of_range());
    }
    Ok(n as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> QueryMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn int_param_defaults_parses_and_bounds() {
        assert_eq!(int_param(&q(&[]), "limit", 50, 1, 200), Ok(50));
        assert_eq!(
            int_param(&q(&[("limit", "20")]), "limit", 50, 1, 200),
            Ok(20)
        );
        for bad in ["0", "201", "abc", "1.5", "", "NaN"] {
            let err = int_param(&q(&[("limit", bad)]), "limit", 50, 1, 200).unwrap_err();
            assert_eq!(err.status, 422, "{bad:?}");
            assert_eq!(err.detail, "limit außerhalb des gültigen Bereichs");
        }
    }
}
