//! The devices that are online right now, per account, and the event stream to each of them.
//!
//! Everything is behind one mutex that is never held across an await: sending to a stream is
//! `try_send` into its bounded buffer. A receiver that does not keep up loses its stream (and
//! reconnects) instead of slowing anybody down.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use chrono::Utc;
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::mpsc;

use super::{Command, Kind, State};

/// More than one household has; a script that invents ids pushes out its own oldest ones.
pub const MAX_DEVICES: usize = 20;
/// Events waiting for one stream. A `devices` event per change of any device fits many times
/// over; a stream that is this far behind is not being read.
const BUFFER: usize = 64;
/// Commands (and transfers) one device is sent per second — a held-down volume key, not a flood.
pub const COMMANDS_PER_SECOND: u32 = 20;

/// The waits of the hub; the tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long a device whose stream ended still counts as online. A phone changing networks
    /// or a proxy cutting the connection reconnects within it, and nobody sees a thing.
    pub grace: Duration,
    /// Between two `: ping` lines, so proxies keep the idle connection and a dead one is noticed.
    pub ping: Duration,
    /// How often an open stream's token is looked up again (see `api::devices`).
    pub recheck: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            grace: Duration::from_secs(20),
            ping: Duration::from_secs(15),
            recheck: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    /// Unknown, gone, or within the grace period without a stream to send to.
    Offline,
    Throttled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransferError {
    TargetOffline,
    Throttled,
    /// No source device, or it plays nothing.
    NothingToTake,
    /// The source is the target.
    AlreadyThere,
}

/// The open stream of a device.
struct Conn {
    tx: mpsc::Sender<Bytes>,
    generation: u64,
    /// Digest of the token the stream was opened with, so a logout can end it.
    token: String,
}

struct Device {
    name: String,
    kind: Kind,
    /// The last report and when it came in (the server's clock, for handing over).
    state: Option<(Arc<State>, Instant)>,
    /// What the other devices were last told about this one; reports are measured against it.
    shown: Option<Arc<State>>,
    conn: Option<Conn>,
    /// Set while the grace period runs: the ticket of the timer that may remove the device.
    lost: Option<u64>,
    /// Last sign of life (a connect, a report), for choosing whom to push out.
    heard: Instant,
    /// The running second of the command throttle and how many it has seen.
    window: (Instant, u32),
}

#[derive(Default)]
struct Account {
    devices: HashMap<String, Device>,
    active: Option<String>,
    /// The device that was last asked to take over. Its next report makes it the active one even
    /// if it took over paused.
    taking_over: Option<String>,
    /// Devices whose buffer ran over in this operation; `Hub::settle` ends their streams.
    overflowed: Vec<String>,
}

#[derive(Serialize)]
pub struct DeviceOut {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// Always true: devices that are offline are not listed. Spelled out so a client's model
    /// does not have to guess.
    pub online: bool,
    pub active: bool,
    pub state: Option<Arc<State>>,
}

#[derive(Serialize)]
pub struct DevicesOut {
    pub devices: Vec<DeviceOut>,
}

/// One server-sent event: `event: <name>` and a single `data:` line of JSON.
fn frame(event: &str, data: &impl Serialize) -> Bytes {
    // serde_json never writes a raw line break, so the JSON is one line.
    let json = serde_json::to_string(data).expect("events are plain data");
    Bytes::from(format!("event: {event}\ndata: {json}\n\n"))
}

impl Account {
    fn list(&self) -> DevicesOut {
        let mut devices: Vec<DeviceOut> = self
            .devices
            .iter()
            .map(|(id, device)| DeviceOut {
                id: id.clone(),
                name: device.name.clone(),
                kind: device.kind,
                online: true,
                active: self.active.as_deref() == Some(id),
                state: device.state.as_ref().map(|(state, _)| state.clone()),
            })
            .collect();
        // A stable order, so a list does not shuffle under the user's finger.
        devices.sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
        DevicesOut { devices }
    }

    /// Queues `bytes` for `id`'s stream. False when it has none or its buffer is full — the
    /// latter also marks the stream to be ended.
    fn send(&mut self, id: &str, bytes: Bytes) -> bool {
        let Some(conn) = self.devices.get(id).and_then(|device| device.conn.as_ref()) else {
            return false;
        };
        match conn.tx.try_send(bytes) {
            Ok(()) => true,
            Err(_) => {
                self.overflowed.push(id.to_owned());
                false
            }
        }
    }

    /// Tells every stream of the account the current list.
    fn broadcast(&mut self) {
        let bytes = frame("devices", &self.list());
        let ids: Vec<String> = self.devices.keys().cloned().collect();
        for id in ids {
            self.send(&id, bytes.clone());
        }
        for device in self.devices.values_mut() {
            device.shown = device.state.as_ref().map(|(state, _)| state.clone());
        }
    }

    fn forget(&mut self, id: &str) -> bool {
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        if self.taking_over.as_deref() == Some(id) {
            self.taking_over = None;
        }
        self.devices.remove(id).is_some()
    }

    /// Makes room for one more: out goes whoever was silent longest, a device without a stream
    /// before one with.
    fn evict_one(&mut self) {
        let victim = self
            .devices
            .iter()
            .min_by_key(|(_, device)| (device.conn.is_some(), device.heard))
            .map(|(id, _)| id.clone());
        if let Some(id) = victim {
            self.forget(&id);
        }
    }

    /// Whether `id` may be sent one more command this second.
    fn admit(&mut self, id: &str, now: Instant) -> bool {
        let Some(device) = self.devices.get_mut(id) else {
            return true;
        };
        if now.duration_since(device.window.0) >= Duration::from_secs(1) {
            device.window = (now, 0);
        }
        device.window.1 += 1;
        device.window.1 <= COMMANDS_PER_SECOND
    }
}

#[derive(Default)]
pub struct Hub {
    accounts: Mutex<HashMap<i32, Account>>,
    /// Numbers streams and grace timers, so a late timer or a dropped old stream can tell that
    /// it is no longer the one that counts.
    counter: AtomicU64,
    timing: Timing,
}

/// A freshly opened stream and its number (for [`Hub::is_current`] and [`Hub::close`]).
pub struct Connection {
    pub events: EventStream,
    pub generation: u64,
}

impl Hub {
    pub fn new(timing: Timing) -> Self {
        Hub {
            timing,
            ..Default::default()
        }
    }

    pub fn timing(&self) -> Timing {
        self.timing
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<i32, Account>> {
        // Nothing in here can leave the map half-written, so a panic elsewhere is no reason to
        // take remote control down with it.
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Ends `id`'s stream, if any, and starts its grace period.
    fn lose(self: &Arc<Self>, user: i32, account: &mut Account, id: &str) {
        let Some(device) = account.devices.get_mut(id) else {
            return;
        };
        // Dropping the sender ends the stream once it has delivered what is queued.
        device.conn = None;
        let ticket = self.next();
        device.lost = Some(ticket);
        let (hub, id, grace) = (self.clone(), id.to_owned(), self.timing.grace);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    tokio::time::sleep(grace).await;
                    hub.expire(user, &id, ticket);
                });
            }
            // No runtime left to wait on (the process is going down): gone at once.
            Err(_) => {
                account.forget(&id);
            }
        }
    }

    /// After every operation that sent something: streams whose buffer ran over are ended.
    fn settle(self: &Arc<Self>, user: i32, accounts: &mut HashMap<i32, Account>) {
        let Some(account) = accounts.get_mut(&user) else {
            return;
        };
        for id in std::mem::take(&mut account.overflowed) {
            self.lose(user, account, &id);
        }
        if account.devices.is_empty() {
            accounts.remove(&user);
        }
    }

    /// The grace period of `ticket` is over: unless the device came back, it is offline now.
    fn expire(self: &Arc<Self>, user: i32, id: &str, ticket: u64) {
        let mut accounts = self.lock();
        let Some(account) = accounts.get_mut(&user) else {
            return;
        };
        if account
            .devices
            .get(id)
            .is_none_or(|device| device.lost != Some(ticket))
        {
            return;
        }
        account.forget(id);
        account.broadcast();
        self.settle(user, &mut accounts);
    }

    /// Opens the stream of device `id`. A stream the device still has is replaced and ends. The
    /// new one starts with the `devices` event; the others hear of it only if something they
    /// see changed — a reconnect within the grace period does not.
    pub fn connect(
        self: &Arc<Self>,
        user: i32,
        id: &str,
        name: String,
        kind: Kind,
        token: String,
    ) -> Connection {
        let (tx, rx) = mpsc::channel(BUFFER);
        let generation = self.next();
        let conn = Conn {
            tx,
            generation,
            token,
        };
        let now = Instant::now();
        let mut accounts = self.lock();
        let account = accounts.entry(user).or_default();
        let changed = match account.devices.get_mut(id) {
            Some(device) => {
                let changed = device.name != name || device.kind != kind;
                device.name = name;
                device.kind = kind;
                device.conn = Some(conn);
                device.lost = None;
                device.heard = now;
                changed
            }
            None => {
                if account.devices.len() >= MAX_DEVICES {
                    account.evict_one();
                }
                account.devices.insert(
                    id.to_owned(),
                    Device {
                        name,
                        kind,
                        state: None,
                        shown: None,
                        conn: Some(conn),
                        lost: None,
                        heard: now,
                        window: (now, 0),
                    },
                );
                true
            }
        };
        if changed {
            account.broadcast();
        } else {
            let list = frame("devices", &account.list());
            account.send(id, list);
        }
        self.settle(user, &mut accounts);
        drop(accounts);
        let mut ping = tokio::time::interval_at(
            tokio::time::Instant::now() + self.timing.ping,
            self.timing.ping,
        );
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Connection {
            events: EventStream {
                rx,
                ping,
                _guard: Guard {
                    hub: self.clone(),
                    user,
                    id: id.to_owned(),
                    generation,
                },
            },
            generation,
        }
    }

    /// The stream `generation` of `id` ended on the client's side.
    fn disconnected(self: &Arc<Self>, user: i32, id: &str, generation: u64) {
        let mut accounts = self.lock();
        let Some(account) = accounts.get_mut(&user) else {
            return;
        };
        let current = account
            .devices
            .get(id)
            .and_then(|device| device.conn.as_ref())
            .is_some_and(|conn| conn.generation == generation);
        // A replaced stream ending is not the device going away.
        if current {
            self.lose(user, account, id);
        }
    }

    /// Whether stream `generation` is still the open stream of `id`.
    pub fn is_current(&self, user: i32, id: &str, generation: u64) -> bool {
        self.lock()
            .get(&user)
            .and_then(|account| account.devices.get(id))
            .and_then(|device| device.conn.as_ref())
            .is_some_and(|conn| conn.generation == generation)
    }

    /// Ends stream `generation` of `id` from the server's side (its token is gone).
    pub fn close(self: &Arc<Self>, user: i32, id: &str, generation: u64) {
        self.disconnected(user, id, generation);
    }

    /// Ends every stream that was opened with this token (a logout).
    pub fn close_token(self: &Arc<Self>, token_digest: &str) {
        let mut accounts = self.lock();
        for (&user, account) in accounts.iter_mut() {
            let ids: Vec<String> = account
                .devices
                .iter()
                .filter(|(_, device)| {
                    device
                        .conn
                        .as_ref()
                        .is_some_and(|c| c.token == token_digest)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                self.lose(user, account, &id);
            }
        }
    }

    /// Ends every stream: the server shuts down, and a graceful shutdown waits for open
    /// responses.
    pub fn close_all(&self) {
        self.lock().clear();
    }

    pub fn list(&self, user: i32) -> DevicesOut {
        self.lock()
            .get(&user)
            .map(Account::list)
            .unwrap_or(DevicesOut { devices: vec![] })
    }

    /// Stores what `id` plays and tells the others if it is worth telling. False for a device
    /// that is not known (it never opened a stream, or it is offline).
    pub fn report(self: &Arc<Self>, user: i32, id: &str, state: State) -> bool {
        let mut accounts = self.lock();
        let Some(account) = accounts.get_mut(&user) else {
            return false;
        };
        let Some(device) = account.devices.get_mut(id) else {
            return false;
        };
        let now = Instant::now();
        let notable = device
            .shown
            .as_ref()
            .is_none_or(|shown| state.differs_notably(shown));
        let playing = state.playing;
        let has_media = state.media.is_some();
        device.state = Some((Arc::new(state), now));
        device.heard = now;
        let takes_over = account.taking_over.as_deref() == Some(id) && has_media;
        let becomes_active = (playing || takes_over) && account.active.as_deref() != Some(id);
        if playing || takes_over {
            account.active = Some(id.to_owned());
            if account.taking_over.as_deref() == Some(id) {
                account.taking_over = None;
            }
        }
        if notable || becomes_active {
            account.broadcast();
        }
        self.settle(user, &mut accounts);
        true
    }

    pub fn command(
        self: &Arc<Self>,
        user: i32,
        target: &str,
        command: &Command,
    ) -> Result<(), SendError> {
        let mut accounts = self.lock();
        let account = accounts.get_mut(&user).ok_or(SendError::Offline)?;
        if !account.admit(target, Instant::now()) {
            return Err(SendError::Throttled);
        }
        let sent = account.send(target, frame("command", command));
        self.settle(user, &mut accounts);
        if sent {
            Ok(())
        } else {
            Err(SendError::Offline)
        }
    }

    /// Hands playback to `target`: it is sent `take_over` with what `from` (or, without one, the
    /// active device) last reported, moved on to now; the source is sent `stop`.
    pub fn transfer(
        self: &Arc<Self>,
        user: i32,
        target: &str,
        from: Option<&str>,
        play: bool,
    ) -> Result<(), TransferError> {
        #[derive(Serialize)]
        struct TakeOver<'a> {
            state: &'a State,
            from: &'a str,
            play: bool,
        }
        let mut accounts = self.lock();
        let account = accounts
            .get_mut(&user)
            .ok_or(TransferError::TargetOffline)?;
        if account
            .devices
            .get(target)
            .is_none_or(|device| device.conn.is_none())
        {
            return Err(TransferError::TargetOffline);
        }
        let source = from
            .map(str::to_owned)
            .or_else(|| account.active.clone())
            .ok_or(TransferError::NothingToTake)?;
        if source == target {
            return Err(TransferError::AlreadyThere);
        }
        let (state, received) = account
            .devices
            .get(&source)
            .and_then(|device| device.state.clone())
            .filter(|(state, _)| state.media.is_some())
            .ok_or(TransferError::NothingToTake)?;
        let now = Instant::now();
        if !account.admit(target, now) {
            return Err(TransferError::Throttled);
        }
        // By the server's own clock since the report came in: a device whose clock is wrong
        // still hands over at the right place.
        let state = state.advanced(now.duration_since(received).as_secs_f64(), Utc::now());
        let take_over = TakeOver {
            state: &state,
            from: &source,
            play,
        };
        let sent = account.send(target, frame("take_over", &take_over));
        if sent {
            account.taking_over = Some(target.to_owned());
            // A source that is between two streams misses this; it stops when it sees the
            // target active.
            account.send(&source, frame("command", &Command::stop(target)));
        }
        self.settle(user, &mut accounts);
        if sent {
            Ok(())
        } else {
            Err(TransferError::TargetOffline)
        }
    }

    /// The device signs off: gone at once, its stream ended.
    pub fn remove(self: &Arc<Self>, user: i32, id: &str) {
        let mut accounts = self.lock();
        let Some(account) = accounts.get_mut(&user) else {
            return;
        };
        if account.forget(id) {
            account.broadcast();
        }
        self.settle(user, &mut accounts);
    }
}

/// Tells the hub when the response body is dropped — the client hung up, or the stream ended.
struct Guard {
    hub: Arc<Hub>,
    user: i32,
    id: String,
    generation: u64,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.hub.disconnected(self.user, &self.id, self.generation);
    }
}

/// The body of `GET /me/devices/events`: the queued events, a `: ping` line at a steady beat
/// (whatever else is sent — a client may watch for exactly that line), and the end once the hub
/// lets go of the stream.
pub struct EventStream {
    rx: mpsc::Receiver<Bytes>,
    ping: tokio::time::Interval,
    _guard: Guard,
}

impl Stream for EventStream {
    type Item = Result<Bytes, std::convert::Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(bytes)) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => match self.ping.poll_tick(cx) {
                Poll::Ready(_) => Poll::Ready(Some(Ok(Bytes::from_static(b": ping\n\n")))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}
