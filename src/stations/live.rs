//! Playing a station: the sender's stream passed on to the client, chunk by chunk.
//!
//! The server sits in between so that a device only ever talks to this server (one address, one
//! login, no mixed content in the browser), and so that it can read the titles the sender
//! weaves into the audio (`stations::icy`). Nothing is buffered beyond the chunk in flight, and
//! the connection to the sender lives inside the response body: when the client goes away, the
//! body is dropped and the sender's connection with it.
//!
//! [`Hub`] is the little that is shared between requests: how many streams each user has open
//! (a cap, so one account cannot make the server hold connections without end) and, per
//! station that somebody is listening to, the title last read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::{Body, Bytes};
use reqwest::header::CONTENT_TYPE;

use super::fetch::{self, Answer, Failed};
use super::icy::{IcyFilter, stream_title};
use super::probe::{is_playlist_type, media_type, parse_url};
use super::store::Station;
use crate::state::AppState;

/// Streams one user may have open at once: a few devices, and the second request a browser
/// sometimes makes for the same `<audio>` element.
pub const MAX_STREAMS_PER_USER: usize = 4;

#[derive(Default)]
struct Shared {
    streams_by_user: HashMap<i32, usize>,
    /// Listeners and the current title, per station somebody is listening to right now.
    live: HashMap<i32, (usize, Option<String>)>,
}

/// What the running streams share. One per process, in `AppState`.
#[derive(Default)]
pub struct Hub {
    shared: Mutex<Shared>,
}

impl Hub {
    /// Registers a stream of `station_id` for `user_id`; `None` when the user has
    /// [`MAX_STREAMS_PER_USER`] open already. The stream counts until the handle is dropped.
    pub fn listen(self: &Arc<Self>, user_id: i32, station_id: i32) -> Option<Listener> {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let open = shared.streams_by_user.entry(user_id).or_insert(0);
        if *open >= MAX_STREAMS_PER_USER {
            return None;
        }
        *open += 1;
        shared.live.entry(station_id).or_insert((0, None)).0 += 1;
        Some(Listener {
            hub: Arc::clone(self),
            user_id,
            station_id,
        })
    }

    /// The title last read from a stream of this station: `None` when nobody is listening,
    /// `Some(None)` when the sender has not named one (yet).
    pub fn now(&self, station_id: i32) -> Option<Option<String>> {
        let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.live.get(&station_id).map(|(_, title)| title.clone())
    }
}

/// One running stream. Dropping it frees the user's slot and, with the last listener, forgets
/// the station's title: an hour-old title must not be shown as what plays now.
pub struct Listener {
    hub: Arc<Hub>,
    user_id: i32,
    station_id: i32,
}

impl Listener {
    fn set_title(&self, title: String) {
        let mut shared = self.hub.shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = shared.live.get_mut(&self.station_id) {
            entry.1 = Some(title).filter(|title| !title.is_empty());
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let mut shared = self.hub.shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(open) = shared.streams_by_user.get_mut(&self.user_id) {
            *open = open.saturating_sub(1);
            if *open == 0 {
                shared.streams_by_user.remove(&self.user_id);
            }
        }
        if let Some(entry) = shared.live.get_mut(&self.station_id) {
            entry.0 = entry.0.saturating_sub(1);
            if entry.0 == 0 {
                shared.live.remove(&self.station_id);
            }
        }
    }
}

/// A sender's answer, ready to be passed on.
pub struct Upstream {
    pub content_type: String,
    pub body: Body,
}

/// Why a station cannot be played right now; both are a 502 to the client.
#[derive(Debug)]
pub struct Unreachable(pub String);

/// Connects to the station's sender and returns its stream as a response body, the ICY
/// metadata taken out and remembered through `listener`.
///
/// Time limits: [`super::probe::CONNECT_TIMEOUT`] until the sender has answered with its headers, and the
/// client's read timeout between two chunks after that — a sender that goes silent ends the
/// stream instead of holding a connection open for ever. There is deliberately no limit on the
/// whole request: a stream has no end.
pub async fn open(
    state: &AppState,
    station: &Station,
    listener: Listener,
) -> Result<Upstream, Unreachable> {
    // The stored address was checked when the station was added, but a host name can point
    // somewhere else today: the same fence applies on every connection.
    let url = parse_url(&station.stream_url).map_err(|e| Unreachable(e.to_owned()))?;
    let response = match fetch::get(state, &url, true).await {
        Ok(response) => response,
        Err(Failed::Refused(why)) => return Err(Unreachable(why.to_owned())),
        Err(Failed::Status(status)) => return Err(Unreachable(format!("HTTP {status}"))),
        Err(Failed::Error(e)) => return Err(Unreachable(e)),
    };
    let header = |name: &str| response.header(name);
    let declared = media_type(header(CONTENT_TYPE.as_str()));
    // A sender that has turned into a web page or a playlist since it was added is not passed
    // on as "audio"; one that names no type is believed to be what it was.
    let content_type = if is_playlist_type(&declared) {
        return Err(Unreachable(format!("liefert {declared} statt Audio")));
    } else if declared.starts_with("audio/") || declared == "application/ogg" {
        declared
    } else if matches!(declared.as_str(), "" | "application/octet-stream") {
        station
            .content_type
            .clone()
            .unwrap_or_else(|| "audio/mpeg".into())
    } else {
        return Err(Unreachable(format!("liefert {declared} statt Audio")));
    };
    let filter = IcyFilter::from_header(header("icy-metaint"));
    Ok(Upstream {
        content_type,
        body: Body::from_stream(pass_on(response, filter, listener)),
    })
}

/// The sender's chunks as a stream of audio. Everything the stream needs lives in its state —
/// the sender's connection and the listener's slot — so dropping it releases both at once.
fn pass_on(
    response: Answer,
    filter: Option<IcyFilter>,
    listener: Listener,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(Some((response, filter, listener)), |state| async move {
        let (mut response, mut filter, listener) = state?;
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                // The sender closed the stream.
                Ok(None) => return None,
                // A stall or a broken connection: end the response as broken too, so the
                // client sees a stream that failed rather than one that finished.
                Err(e) => {
                    return Some((Err(std::io::Error::other(e)), None));
                }
            };
            let audio = match filter.as_mut() {
                None => chunk,
                Some(filter) => {
                    let mut audio = Vec::with_capacity(chunk.len());
                    for block in filter.feed(&chunk, &mut audio) {
                        if let Some(title) = stream_title(&block) {
                            listener.set_title(title);
                        }
                    }
                    Bytes::from(audio)
                }
            };
            // A chunk that was all metadata has nothing to send; wait for the next one.
            if !audio.is_empty() {
                return Some((Ok(audio), Some((response, filter, listener))));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_has_a_fixed_number_of_slots_and_gets_them_back() {
        let hub = Arc::new(Hub::default());
        let mut open: Vec<Listener> = (0..MAX_STREAMS_PER_USER)
            .map(|_| hub.listen(1, 10).expect("a free slot"))
            .collect();
        assert!(hub.listen(1, 11).is_none());
        // Another user is not affected.
        assert!(hub.listen(2, 20).is_some());
        open.pop();
        assert!(hub.listen(1, 11).is_some());
    }

    #[test]
    fn the_title_lives_as_long_as_somebody_listens() {
        let hub = Arc::new(Hub::default());
        assert_eq!(hub.now(10), None);
        let first = hub.listen(1, 10).unwrap();
        let second = hub.listen(2, 10).unwrap();
        assert_eq!(hub.now(10), Some(None));
        first.set_title("Kraftwerk - Autobahn".into());
        assert_eq!(hub.now(10), Some(Some("Kraftwerk - Autobahn".into())));
        drop(first);
        assert_eq!(hub.now(10), Some(Some("Kraftwerk - Autobahn".into())));
        // An empty title (a jingle) clears the last one.
        second.set_title(String::new());
        assert_eq!(hub.now(10), Some(None));
        drop(second);
        assert_eq!(hub.now(10), None);
    }
}
