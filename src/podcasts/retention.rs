//! How many episodes of a podcast stay downloaded, and the scheduled refresh that applies it
//! (legacy `podcasts/retention.ts`).

use std::path::{Path, PathBuf};

use sqlx::PgPool;

use super::download::download_episode;
use super::paths::prune_empty_podcast_dir;
use super::store::refresh;

pub const KEEP_MIN: i32 = 0;
pub const KEEP_MAX: i32 = 50;

/// Into `KEEP_MIN..=KEEP_MAX`, fractions cut off.
pub fn clamp_keep(value: f64) -> i32 {
    if value.is_nan() {
        return KEEP_MIN;
    }
    value
        .trunc()
        .clamp(f64::from(KEEP_MIN), f64::from(KEEP_MAX)) as i32
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionResult {
    pub downloaded: u32,
    pub freed: u32,
    pub failed: u32,
}

pub async fn get_keep_default(db: &PgPool) -> Result<i32, sqlx::Error> {
    let keep: Option<i32> =
        sqlx::query_scalar("SELECT podcast_keep_episodes FROM library_config WHERE id = 1")
            .fetch_optional(db)
            .await?;
    Ok(keep.unwrap_or(0))
}

pub async fn set_keep_default(db: &PgPool, keep: f64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO library_config (id, podcast_keep_episodes) VALUES (1, $1)
         ON CONFLICT (id) DO UPDATE SET podcast_keep_episodes = $1, updated_at = now()",
    )
    .bind(clamp_keep(keep))
    .execute(db)
    .await?;
    Ok(())
}

/// `None` falls back to the default.
pub async fn set_keep_for_podcast(
    db: &PgPool,
    podcast_id: i32,
    keep: Option<f64>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE items SET keep_episodes = $2 WHERE id = $1 AND kind = 'podcast'")
        .bind(podcast_id)
        .bind(keep.map(clamp_keep))
        .execute(db)
        .await?;
    Ok(())
}

pub async fn effective_keep(db: &PgPool, keep_episodes: Option<i32>) -> Result<i32, sqlx::Error> {
    match keep_episodes {
        Some(keep) => Ok(keep),
        None => get_keep_default(db).await,
    }
}

async fn remove_download(
    db: &PgPool,
    episode_id: i32,
    podcasts_dir: &Path,
) -> Result<u32, sqlx::Error> {
    let paths: Vec<String> =
        sqlx::query_scalar("DELETE FROM tracks WHERE item_id = $1 RETURNING path")
            .bind(episode_id)
            .fetch_all(db)
            .await?;
    for path in paths.iter().map(PathBuf::from) {
        let _ = tokio::fs::remove_file(&path).await;
        prune_empty_podcast_dir(&path, podcasts_dir).await;
    }
    Ok(paths.len() as u32)
}

/// The newest `keep` episodes downloaded, every older download deleted. A failed download is
/// counted, not fatal.
pub async fn apply_retention(
    db: &PgPool,
    http: &reqwest::Client,
    podcast_id: i32,
    keep: i32,
    podcasts_dir: &Path,
) -> Result<RetentionResult, sqlx::Error> {
    let ordered: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT episode.id, EXISTS (SELECT 1 FROM tracks WHERE tracks.item_id = episode.id)
         FROM items AS episode
         WHERE episode.parent_id = $1 AND episode.kind = 'episode'
         ORDER BY episode.published_at DESC NULLS LAST, episode.id DESC",
    )
    .bind(podcast_id)
    .fetch_all(db)
    .await?;
    let keep = usize::try_from(keep.max(0)).unwrap_or(0);
    let mut result = RetentionResult::default();
    for (index, (episode_id, downloaded)) in ordered.into_iter().enumerate() {
        if index < keep {
            if downloaded {
                continue;
            }
            match download_episode(db, http, episode_id, podcasts_dir).await {
                Ok(_) => result.downloaded += 1,
                Err(e) => {
                    tracing::warn!("Folge {episode_id} nicht geladen: {e}");
                    result.failed += 1;
                }
            }
        } else if downloaded {
            result.freed += remove_download(db, episode_id, podcasts_dir).await?;
        }
    }
    Ok(result)
}

/// Retention with the podcast's own number or the default; nothing for anything but a podcast.
pub async fn retain_for_podcast(
    db: &PgPool,
    http: &reqwest::Client,
    podcast_id: i32,
    podcasts_dir: &Path,
) -> Result<RetentionResult, sqlx::Error> {
    let podcast: Option<Option<i32>> =
        sqlx::query_scalar("SELECT keep_episodes FROM items WHERE id = $1 AND kind = 'podcast'")
            .bind(podcast_id)
            .fetch_optional(db)
            .await?;
    let Some(keep_episodes) = podcast else {
        return Ok(RetentionResult::default());
    };
    let keep = effective_keep(db, keep_episodes).await?;
    apply_retention(db, http, podcast_id, keep, podcasts_dir).await
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshRunResult {
    pub podcasts: u32,
    pub new_episodes: u32,
    pub downloaded: u32,
    pub freed: u32,
    pub failed: u32,
    pub errors: Vec<String>,
}

/// One feed that is gone must not take the whole run down, so every podcast is handled on its own
/// — but the reason travels back in `errors` instead of being dropped. A silent run is
/// indistinguishable from a run that never happened.
pub async fn refresh_all_and_retain(
    db: &PgPool,
    http: &reqwest::Client,
    covers_dir: &Path,
    podcasts_dir: &Path,
) -> Result<RefreshRunResult, sqlx::Error> {
    let ids: Vec<i32> =
        sqlx::query_scalar("SELECT id FROM items WHERE kind = 'podcast' ORDER BY id")
            .fetch_all(db)
            .await?;
    let mut totals = RefreshRunResult::default();
    for id in ids {
        totals.podcasts += 1;
        match refresh(db, http, id, Some(covers_dir)).await {
            Ok(synced) => totals.new_episodes += synced.new_episodes,
            Err(e) => {
                totals.failed += 1;
                totals.errors.push(format!("Podcast {id}: {e}"));
                continue;
            }
        }
        let result = retain_for_podcast(db, http, id, podcasts_dir).await?;
        totals.downloaded += result.downloaded;
        totals.freed += result.freed;
        totals.failed += result.failed;
    }
    Ok(totals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_sensible_number() {
        assert_eq!(clamp_keep(3.0), 3);
        assert_eq!(clamp_keep(f64::from(KEEP_MIN)), KEEP_MIN);
        assert_eq!(clamp_keep(f64::from(KEEP_MAX)), KEEP_MAX);
    }

    #[test]
    fn pulls_anything_outside_the_range_back_in() {
        assert_eq!(clamp_keep(-5.0), KEEP_MIN);
        assert_eq!(clamp_keep(999.0), KEEP_MAX);
    }

    #[test]
    fn refuses_to_make_a_fraction_of_an_episode() {
        assert_eq!(clamp_keep(2.9), 2);
    }
}
