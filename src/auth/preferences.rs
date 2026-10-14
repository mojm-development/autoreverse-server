//! A user's playback preferences: speed and skip distances (legacy `auth/preferences.ts`).

use serde::Serialize;
use sqlx::PgPool;

pub const SPEED_MIN: f64 = 0.5;
pub const SPEED_MAX: f64 = 4.0;
pub const SKIP_MIN: i32 = 1;
pub const SKIP_MAX: i32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, sqlx::FromRow)]
pub struct Preferences {
    #[serde(serialize_with = "crate::serialize::js_number")]
    pub playback_speed: f64,
    pub skip_back: i32,
    pub skip_forward: i32,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            playback_speed: 1.0,
            skip_back: 30,
            skip_forward: 15,
        }
    }
}

pub fn clamp_speed(v: f64) -> f64 {
    v.clamp(SPEED_MIN, SPEED_MAX)
}

/// Whole seconds within the bounds; fractions are cut off, not rounded (`Math.trunc`).
pub fn clamp_skip(v: f64) -> i32 {
    (v.trunc() as i64).clamp(SKIP_MIN as i64, SKIP_MAX as i64) as i32
}

pub async fn get_preferences(db: &PgPool, user_id: i32) -> Result<Preferences, sqlx::Error> {
    let row: Option<Preferences> =
        sqlx::query_as("SELECT playback_speed, skip_back, skip_forward FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    Ok(row.unwrap_or_default())
}

/// Stores the clamped values and returns what was stored. Callers check the bounds themselves
/// where an out-of-range value is an error (the API); NaN must not get this far.
pub async fn set_preferences(
    db: &PgPool,
    user_id: i32,
    playback_speed: f64,
    skip_back: f64,
    skip_forward: f64,
) -> Result<Preferences, sqlx::Error> {
    let row: Option<Preferences> = sqlx::query_as(
        "UPDATE users SET playback_speed = $2, skip_back = $3, skip_forward = $4
         WHERE id = $1
         RETURNING playback_speed, skip_back, skip_forward",
    )
    .bind(user_id)
    .bind(clamp_speed(playback_speed))
    .bind(clamp_skip(skip_back))
    .bind(clamp_skip(skip_forward))
    .fetch_optional(db)
    .await?;
    Ok(row.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_speed_and_skips() {
        assert_eq!(clamp_speed(0.1), 0.5);
        assert_eq!(clamp_speed(1.25), 1.25);
        assert_eq!(clamp_speed(9.0), 4.0);
        assert_eq!(clamp_skip(0.0), 1);
        assert_eq!(clamp_skip(29.9), 29);
        assert_eq!(clamp_skip(1e9), 300);
        assert_eq!(clamp_skip(-5.0), 1);
    }
}
