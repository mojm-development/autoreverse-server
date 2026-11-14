//! Logos for the hits of the directory search, served by this server (`GET
//! /radio/stations/search/logo?id=`), so that a client showing a list of hits loads nothing from
//! the dozens of servers the directory names.
//!
//! The route never takes an address. The server remembers from the directory's answers which id
//! has which logo and which homepage (`known`), asks the directory once for an id it does not
//! know (any more), and fetches the logo through the fenced client like everything else a
//! station names (`probe::fetch_logo`). Where the directory names no logo, or one that cannot
//! be had, the icon of the station's homepage stands in (`site_icon`) — once per homepage,
//! however many stations name it: a broadcaster's ten channels share one page. What came of it — the picture, or that there is none to be had — is
//! kept in memory for a while (`slots`): a list of 25 rows scrolled up and down must not fetch
//! 25 logos again and again, and a logo server that is down must not cost every request its
//! full timeout. Requests for the same id share one fetch, and only a few stations are worked
//! on at once.
//!
//! Nothing here is stored on disk: the logo of a station somebody adds goes to
//! `logos::attach`, everything else is forgotten with the process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OnceCell, Semaphore};

use super::directory::{self, DirectoryStation, is_directory_id};
use super::logos::{self, Picture};
use super::probe;
use super::site_icon;
use crate::state::AppState;

/// Ids whose logo address is remembered: a few dozen searches' worth.
const MAX_KNOWN: usize = 2000;
/// Fetched logos (and failures) kept at once, and what the pictures may weigh together.
const MAX_KEPT: usize = 400;
const MAX_KEPT_BYTES: usize = 48 * 1024 * 1024;
/// Stations whose logo is looked for at the same time, over all users: a hit list asks for 25
/// at once, and each may take a handful of requests one after the other.
const MAX_FETCHES: usize = 6;
/// Pictures scaled at the same time (CPU work).
const MAX_SCALING: usize = 4;
/// How long a station without a logo waits until `logos::backfill` tries it again.
const BACKFILL_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// For the directory's answer, for the logo, and for each request to a homepage.
    pub fetch: Duration,
    /// For everything a homepage is asked, together.
    pub site: Duration,
    /// How long an id's logo address and a fetched logo are kept.
    pub keep: Duration,
    /// How long it is remembered that a logo could not be had.
    pub keep_failure: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            fetch: Duration::from_secs(5),
            site: Duration::from_secs(8),
            keep: Duration::from_secs(6 * 60 * 60),
            keep_failure: Duration::from_secs(10 * 60),
        }
    }
}

/// Where the directory says a station's logo may come from: its logo address, its homepage,
/// or neither.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Named {
    logo_url: Option<String>,
    homepage: Option<String>,
}

impl From<&DirectoryStation> for Named {
    fn from(station: &DirectoryStation) -> Self {
        Named {
            logo_url: station.logo_url.clone(),
            homepage: station.homepage.clone(),
        }
    }
}

struct Known {
    named: Named,
    at: Instant,
}

/// A fetched logo and the sizes it was asked for so far.
pub struct Logo {
    pub original: Picture,
    /// By edge length; `None` where the original is that small already.
    scaled: Mutex<HashMap<u32, Option<Picture>>>,
}

impl Logo {
    fn new(original: Picture) -> Self {
        Logo {
            original,
            scaled: Mutex::default(),
        }
    }

    fn bytes(&self) -> usize {
        let scaled = self.scaled.lock().expect("logo sizes");
        self.original.data.len()
            + scaled
                .values()
                .flatten()
                .map(|picture| picture.data.len())
                .sum::<usize>()
    }
}

struct Outcome {
    at: Instant,
    logo: Option<Arc<Logo>>,
}

/// One fetch — an id's, or a homepage's: empty while it runs, which is what later requests for
/// the same wait on.
#[derive(Default)]
struct Slot {
    outcome: OnceCell<Outcome>,
}

pub struct SearchLogos {
    timing: Timing,
    known: Mutex<HashMap<String, Known>>,
    /// By directory id, and by homepage ([`site_key`]). A logo that came from a homepage is in
    /// both, which only makes the weighing err on the careful side.
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    fetches: Semaphore,
    scaling: Semaphore,
    /// When a stored station without a logo was last tried, by station id.
    backfilled: Mutex<HashMap<i32, Instant>>,
}

impl Default for SearchLogos {
    fn default() -> Self {
        SearchLogos::new(Timing::default())
    }
}

impl SearchLogos {
    pub fn new(timing: Timing) -> Self {
        SearchLogos {
            timing,
            known: Mutex::default(),
            slots: Mutex::default(),
            fetches: Semaphore::new(MAX_FETCHES),
            scaling: Semaphore::new(MAX_SCALING),
            backfilled: Mutex::default(),
        }
    }

    /// Notes what a directory answer says about the logos and homepages of its stations.
    pub fn remember(&self, stations: &[DirectoryStation]) {
        let now = Instant::now();
        let mut known = self.known.lock().expect("known logos");
        for station in stations {
            known.insert(
                station.directory_id.to_ascii_lowercase(),
                Known {
                    named: station.into(),
                    at: now,
                },
            );
        }
        if known.len() <= MAX_KNOWN {
            return;
        }
        known.retain(|_, entry| entry.at.elapsed() < self.timing.keep);
        if known.len() > MAX_KNOWN {
            let mut ages: Vec<Instant> = known.values().map(|entry| entry.at).collect();
            ages.sort_unstable();
            // Entries of one answer share their time, so this may drop a few more than needed;
            // the directory is asked again for those.
            let newest_dropped = ages[ages.len() - MAX_KNOWN - 1];
            known.retain(|_, entry| entry.at > newest_dropped);
        }
    }

    /// What the directory named for this id; `None`: not known (any more).
    fn known(&self, id: &str) -> Option<Named> {
        let known = self.known.lock().expect("known logos");
        known
            .get(id)
            .filter(|entry| entry.at.elapsed() < self.timing.keep)
            .map(|entry| entry.named.clone())
    }

    fn expired(&self, slot: &Slot) -> bool {
        slot.outcome.get().is_some_and(|outcome| {
            let keep = if outcome.logo.is_some() {
                self.timing.keep
            } else {
                self.timing.keep_failure
            };
            outcome.at.elapsed() >= keep
        })
    }

    /// The slot of `id` (or of a homepage): the one that is there while it is fresh or still being fetched, else a
    /// new one — making room first, the oldest outcomes going.
    fn slot(&self, id: &str) -> Arc<Slot> {
        let mut slots = self.slots.lock().expect("logo slots");
        if let Some(slot) = slots.get(id)
            && !self.expired(slot)
        {
            return slot.clone();
        }
        slots.retain(|_, slot| !self.expired(slot));
        let weight = |slot: &Slot| {
            let logo = slot.outcome.get().and_then(|outcome| outcome.logo.as_ref());
            logo.map_or(0, |logo| logo.bytes())
        };
        let mut bytes: usize = slots.values().map(|slot| weight(slot)).sum();
        while slots.len() >= MAX_KEPT || bytes > MAX_KEPT_BYTES {
            let oldest = slots
                .iter()
                .filter_map(|(id, slot)| Some((slot.outcome.get()?.at, id)))
                .min()
                .map(|(_, id)| id.clone());
            // Only fetches that still run are left: they are bounded by who waits for them.
            let Some(oldest) = oldest else { break };
            if let Some(slot) = slots.remove(&oldest) {
                bytes -= weight(&slot);
            }
        }
        let slot = Arc::<Slot>::default();
        slots.insert(id.to_owned(), slot.clone());
        slot
    }

    /// The icon of the page at `homepage`, from memory or fetched now — by whoever asks first;
    /// everybody else waits for that one. The caller holds a permit of `fetches`.
    async fn site(&self, state: &AppState, homepage: &str) -> Option<Arc<Logo>> {
        let slot = self.slot(&site_key(homepage)?);
        let outcome = slot
            .outcome
            .get_or_init(|| async {
                let timing = &self.timing;
                let picture = site_icon::fetch(state, homepage, timing.fetch, timing.site).await;
                Outcome {
                    at: Instant::now(),
                    logo: picture.map(Logo::new).map(Arc::new),
                }
            })
            .await;
        outcome.logo.clone()
    }

    async fn fetch(&self, state: &AppState, id: &str) -> Option<Arc<Logo>> {
        let _permit = self.fetches.acquire().await.ok()?;
        let named = match self.known(id) {
            Some(named) => named,
            None => {
                let asked = directory::lookup(&state.http, &state.config.radio_directory_url, id);
                let found = tokio::time::timeout(self.timing.fetch, asked)
                    .await
                    .ok()?
                    .ok()?;
                // A station the directory does not know has no logo either; both are noted, so
                // the directory is asked once.
                let named = found.as_ref().map(Named::from).unwrap_or_default();
                self.known.lock().expect("known logos").insert(
                    id.to_owned(),
                    Known {
                        named: named.clone(),
                        at: Instant::now(),
                    },
                );
                named
            }
        };
        // The directory's logo is the station's own choice of picture and costs one request,
        // so it comes first; the homepage is looked at only when that yields nothing.
        if let Some(logo_url) = &named.logo_url
            && let Some(original) = probe::fetch_logo(state, logo_url, self.timing.fetch).await
        {
            return Some(Arc::new(Logo::new(original)));
        }
        self.site(state, named.homepage.as_deref()?).await
    }

    /// The logo of the directory's station `directory_id`, from memory or fetched now. `None`
    /// when neither the directory's logo nor the homepage's icons can be had — fetched in time,
    /// small enough, a picture — and on a server that does not ask the directory.
    pub async fn get(state: &AppState, directory_id: &str) -> Option<Arc<Logo>> {
        let id = directory_id.trim().to_ascii_lowercase();
        if !state.config.radio_directory || !is_directory_id(&id) {
            return None;
        }
        let state = state.clone();
        // In a task of its own: a client that scrolls on hangs up, and the fetch it started
        // should still end in the memory instead of being started anew by the next one.
        let fetched = tokio::spawn(async move {
            let logos = &state.radio_logos;
            let slot = logos.slot(&id);
            let outcome = slot
                .outcome
                .get_or_init(|| async {
                    let logo = logos.fetch(&state, &id).await;
                    Outcome {
                        at: Instant::now(),
                        logo,
                    }
                })
                .await;
            outcome.logo.clone()
        });
        fetched.await.ok().flatten()
    }

    /// The icon of the page at `homepage`, for a station that has no directory entry to go by
    /// (added by its address). Remembered and bounded like the logos of the hits.
    pub async fn of_homepage(state: &AppState, homepage: &str) -> Option<Arc<Logo>> {
        let logos = &state.radio_logos;
        let _permit = logos.fetches.acquire().await.ok()?;
        logos.site(state, homepage).await
    }

    /// `logo` as it goes out: the original, or with a `size` no larger than `size`×`size`.
    pub async fn picture(&self, logo: &Arc<Logo>, size: Option<u32>) -> Picture {
        let Some(size) = size else {
            return logo.original.clone();
        };
        let ready = logo.scaled.lock().expect("logo sizes").get(&size).cloned();
        let scaled = match ready {
            Some(scaled) => scaled,
            None => {
                let _permit = self.scaling.acquire().await;
                let source = logo.clone();
                let scaled =
                    tokio::task::spawn_blocking(move || logos::scaled(&source.original, size))
                        .await
                        .ok()
                        .flatten();
                let mut sizes = logo.scaled.lock().expect("logo sizes");
                sizes.insert(size, scaled.clone());
                scaled
            }
        };
        scaled.unwrap_or_else(|| logo.original.clone())
    }

    /// Whether the stored station `station_id` is due for another try at its logo; saying yes
    /// starts its wait anew.
    pub fn claim(&self, station_id: i32) -> bool {
        let mut tried = self.backfilled.lock().expect("backfilled logos");
        if tried
            .get(&station_id)
            .is_some_and(|at| at.elapsed() < BACKFILL_EVERY)
        {
            return false;
        }
        if tried.len() >= 4 * MAX_KNOWN {
            tried.retain(|_, at| at.elapsed() < BACKFILL_EVERY);
        }
        tried.insert(station_id, Instant::now());
        true
    }
}

/// The key of a homepage among the slots. Directory ids are hex digits and dashes, so the two
/// kinds cannot meet. `None` for what is no web address.
fn site_key(homepage: &str) -> Option<String> {
    let mut url = probe::parse_url(homepage).ok()?;
    url.set_fragment(None);
    Some(format!("site {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(logo_url: Option<&str>, homepage: Option<&str>) -> Named {
        Named {
            logo_url: logo_url.map(str::to_owned),
            homepage: homepage.map(str::to_owned),
        }
    }

    fn station(id: &str, logo_url: Option<&str>) -> DirectoryStation {
        DirectoryStation {
            directory_id: id.into(),
            name: "Sender".into(),
            stream_url: "http://sender.example/live".into(),
            homepage: None,
            country: None,
            tags: Vec::new(),
            codec: None,
            bitrate: None,
            logo_url: logo_url.map(str::to_owned),
            hls: false,
        }
    }

    #[test]
    fn remembers_what_the_directory_said_in_any_spelling_of_the_id() {
        let logos = SearchLogos::default();
        let mut with_page = station("abcdef00-2", None);
        with_page.homepage = Some("https://sender.example/".into());
        logos.remember(&[
            station("ABCDEF00-1", Some("http://logo.example/a.png")),
            with_page,
            station("abcdef00-3", None),
        ]);
        assert_eq!(
            logos.known("abcdef00-1"),
            Some(named(Some("http://logo.example/a.png"), None))
        );
        assert_eq!(
            logos.known("abcdef00-2"),
            Some(named(None, Some("https://sender.example/")))
        );
        assert_eq!(logos.known("abcdef00-3"), Some(Named::default()));
        assert_eq!(logos.known("abcdef00-4"), None);
    }

    #[test]
    fn forgets_the_oldest_ids_beyond_its_bound_and_all_of_them_in_time() {
        let logos = SearchLogos::default();
        logos.remember(&[station("00000000-old", None)]);
        std::thread::sleep(Duration::from_millis(2));
        let many: Vec<DirectoryStation> = (0..MAX_KNOWN)
            .map(|i| station(&format!("{i:08x}"), None))
            .collect();
        logos.remember(&many);
        assert_eq!(logos.known("00000000-old"), None);
        assert_eq!(logos.known("00000000"), Some(Named::default()));
        assert_eq!(logos.known.lock().unwrap().len(), MAX_KNOWN);

        let brief = SearchLogos::new(Timing {
            keep: Duration::ZERO,
            ..Timing::default()
        });
        brief.remember(&[station("abcdef00-1", None)]);
        assert_eq!(brief.known("abcdef00-1"), None);
    }

    #[test]
    fn makes_room_for_a_new_slot_by_dropping_the_oldest_outcome() {
        let logos = SearchLogos::default();
        let done = |id: &str| {
            let slot = logos.slot(id);
            let outcome = Outcome {
                at: Instant::now(),
                logo: None,
            };
            assert!(slot.outcome.set(outcome).is_ok());
            std::thread::sleep(Duration::from_micros(50));
        };
        for i in 0..MAX_KEPT {
            done(&format!("{i:08x}"));
        }
        let running = logos.slot("ffffffff");
        let slots = logos.slots.lock().unwrap();
        assert_eq!(slots.len(), MAX_KEPT);
        assert!(!slots.contains_key("00000000"));
        assert!(slots.contains_key("00000001"));
        assert!(Arc::ptr_eq(&slots["ffffffff"], &running));
    }

    #[test]
    fn a_failure_is_kept_for_a_shorter_time_than_a_logo() {
        let logos = SearchLogos::new(Timing {
            keep_failure: Duration::ZERO,
            ..Timing::default()
        });
        let slot = logos.slot("abcdef00");
        // Still being fetched: later requests join it.
        assert!(Arc::ptr_eq(&logos.slot("abcdef00"), &slot));
        let failed = Outcome {
            at: Instant::now(),
            logo: None,
        };
        assert!(slot.outcome.set(failed).is_ok());
        assert!(!Arc::ptr_eq(&logos.slot("abcdef00"), &slot));

        let kept = logos.slot("abcdef01");
        let picture = Picture {
            extension: "png",
            content_type: "image/png",
            data: vec![0; 10].into(),
        };
        let logo = Logo::new(picture);
        assert_eq!(logo.bytes(), 10);
        let fetched = Outcome {
            at: Instant::now(),
            logo: Some(Arc::new(logo)),
        };
        assert!(kept.outcome.set(fetched).is_ok());
        assert!(Arc::ptr_eq(&logos.slot("abcdef01"), &kept));
    }

    #[test]
    fn a_homepage_has_one_slot_however_it_is_spelled() {
        assert_eq!(
            site_key(" HTTPS://Sender.Example#start "),
            Some("site https://sender.example/".into())
        );
        assert_eq!(
            site_key("https://sender.example/"),
            site_key("https://sender.example")
        );
        // Another scheme may be another page.
        assert_ne!(
            site_key("http://sender.example/"),
            site_key("https://sender.example/")
        );
        for none in ["", "sender.example", "ftp://sender.example/", "file:///etc"] {
            assert_eq!(site_key(none), None, "{none}");
        }
        // No id can be taken for a homepage.
        assert!(!is_directory_id(&site_key("https://a.example/").unwrap()));
    }

    #[test]
    fn a_stored_station_is_tried_once_in_a_while() {
        let logos = SearchLogos::default();
        assert!(logos.claim(7));
        assert!(!logos.claim(7));
        assert!(logos.claim(8));
    }
}
