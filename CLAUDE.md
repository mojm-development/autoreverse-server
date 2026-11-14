# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Autoreverse Server: a self-hosted media server for audiobooks, music and podcasts. One Rust
binary (`autoreverse-server`: axum + tokio + sqlx/Postgres) serves both a JSON API for the native
clients (the iOS app) and a server-rendered German web UI (askama templates, htmx, small vanilla
JS modules). ffprobe/ffmpeg are external processes; nothing else runs beside it.

The code is a port of an earlier SvelteKit/TypeScript server, which in turn ported a Python/FastAPI
one. Comments call the TS version **"legacy"** (`legacy routes/items/+server.ts`, `legacy
scanState`) and sometimes cite review IDs (`C-2`, `I-6`, `Ruling 8`). Legacy is not part of this
repository; the references are kept as history. The JSON wire format is deliberately kept
byte-compatible with legacy — snake_case fields, German `detail` texts, status codes — except where
a comment marks an intentional divergence.

## Commands

```sh
make setup                # once: .env, dev-library/, Postgres
make dev                  # cargo run on http://localhost:3000 (starts Postgres first)
make help                 # everything else

docker compose up -d postgres   # dev Postgres on host port 5434 (autoreverse/autoreverse)
cargo run                       # serves; reads .env via dotenvy
cargo run -- set-password <name> [--admin]   # password from a prompt or the first stdin line

cargo test                                  # unit + integration (needs the Postgres on :5434)
cargo test --lib scanner::tags              # one unit module
cargo test --test integration podcasts::    # one integration module
cargo clippy --all-targets -- -D warnings
cargo fmt
node --test 'tests/js/*.test.mjs'           # JS unit tests of static/js

e2e/run-in-vm.sh [tests/login.e2e.ts]   # Playwright, browser in a Tart VM on the build Mac
docker build -t autoreverse-server .        # the release image
```

`.cargo/config.toml` points `DATABASE_URL` at the compose Postgres, so `cargo test` needs no
setup beyond `docker compose up -d postgres`; an exported `DATABASE_URL` wins. `ffprobe`/`ffmpeg`
must be on PATH (m4b chapters, loudness measuring; `tests/integration/scan_api.rs`,
`scanner_folders.rs` and `loudness.rs` shell out to `ffmpeg` to build fixtures).

**E2E never runs in a browser on the development machine** (macOS or Linux). `e2e/run-in-vm.sh` builds and starts the server
locally on :3100 against a fresh database `autoreverse_e2e` (seeded from `e2e/seed.sql`), boots the
Tart VM `autoreverse-e2e` on the build Mac, runs Playwright there and stops VM and server again. Only one
VM at a time on the build Mac; the script waits. `e2e/design/compare.sh` screenshots legacy and Rust side
by side the same way; it needs a repository with the legacy commit (`LEGACY_REPO`, `LEGACY_REV`).

## Architecture

### Modules

```
src/
  main.rs       CLI dispatch (serve | set-password | healthcheck), migrations, first admin, serve
  app.rs        app(state) -> Router: the router factory, plus the cross-cutting middleware
  state.rs      AppState: pool, config, login throttle + semaphore, scan status, HTTP client,
                the hubs of radio streams and of remote-controlled devices, the logos of
                the station search
  config.rs     env → Config; find_static_dir
  db.rs         pool, embedded migrations, refuse a legacy (Drizzle) database
  error.rs      ApiError / WebError and their responses
  serialize.rs  wire format: rows in, snake_case JSON out, to_iso()
  api/          JSON routes, one file per area (items, tracks, playback, podcasts, users, …)
  web/          HTML routes, one file per area, plus layout, format helpers, error shell
  auth/         sessions/extractors, tokens, argon2 passwords, throttle, bootstrap
  library/      queries and mutations shared by api/ and web/ (progress, playlists, metadata,
                lyrics, top lists …)
  scanner/      the scan: walk, tags (lofty), chapters (ffprobe), covers, store, status,
                loudness (tags, the measuring pass)
  podcasts/     feeds (feed-rs), directory search, downloads, retention, the hourly job
  stations/     internet radio: directory (radio-browser.info), probe, store, live proxy, ICY,
                logos (stored ones, those of the search hits, the icons of a homepage)
  devices/      remote control: wire shapes and their checks (mod.rs), the hub of online
                devices and their event streams (hub.rs)
  streaming/    HTTP Range responses
templates/      askama: base.html, layouts/, pages/, partials/ — compiled into the binary
static/         css/ (tokens.css, app.css, components.css, pages/*.css), js/, icons
migrations/     sqlx migrations — compiled into the binary, applied at every start
tests/          integration/ (one test binary, a module per area), js/, fixtures/
e2e/            Playwright specs, seed, run-in-vm.sh, design/ (parity tooling)
```

`api/` and `web/` are thin: they parse, guard and render. Queries live in `library/`,
`scanner/` and `podcasts/`, and web handlers call those **directly**, never the JSON API over HTTP.
Only a few interactive pieces (`static/js/…`) `fetch()` the API from the browser.

### Router factory

`app::app(state) -> Router` is the whole server: `api::router()` merged with `web::router()`,
`static/` as the fallback service, then the middleware (error shell for web pages, cross-site
form check, trailing-slash 308, panic → 500 `{detail}`, tracing). Each area exposes its own
`router()`. Integration tests build the same router through `tests/integration/common.rs`
(`TestApp`) and drive it with `tower::ServiceExt::oneshot` — no socket, no port.

### Errors

API handlers return `ApiResult<T>`; fail with `ApiError::new(status, "<deutscher Text>")` (helpers
like `ApiError::unprocessable`, `with_retry_after` for 429/503). Every API error body is
`{ "detail": "<German>" }`, including panics and internal errors (logged, sent as "Interner
Serverfehler"). Parse request bodies and queries with the extractors in `api/validate.rs`
(`ApiJson`, query helpers) so malformed input becomes a 422 with a German `detail`, never axum's
English plain-text rejection or a 500.

Web handlers return `WebResult<T>`: `WebError::Redirect` (303, e.g. no session),
`WebError::Page(status, message)` for the error page inside the app or settings frame, and
`WebError::Internal`. An `ApiError` converts into `WebError::Page`.

### Auth

One resolver for both worlds: a `Bearer` header first, else the `autoreverse_session` cookie.
Guards are extractors in `auth/session.rs`: `ApiUser` / `ApiAdmin` answer 401/403 with
`{detail}`, `WebUser` / `WebAdmin` redirect (to `/login`, or `/library` for a non-admin).
`Session` is the optional variant. Passwords are argon2id with a concurrency semaphore and a
per-process login throttle by name, plus one by client address that counts only public peers
(behind a proxy all arrive from its private address; `X-Forwarded-For` is not trusted).
Outgoing requests for URLs a feed names go through `podcasts::net`'s fenced client, which
reaches no private, loopback or link-local address unless the feed itself lives in the LAN. At start `ensure_first_admin` creates the admin
from `AUTOREVERSE_ADMIN_USER`/`_PASSWORD`, only while the `users` table is empty;
`autoreverse-server set-password` is the rescue path. A form POST whose `Origin` is not the
server's own (`ORIGIN`, else `http://<Host>`) is refused, as SvelteKit did — behind a TLS proxy
`ORIGIN` must be set. Beyond legacy, any POST/PUT/PATCH/DELETE without a bearer token that the
browser marks `Sec-Fetch-Site: cross-site|same-site` is refused too. `auth_tokens.value` holds the
SHA-256 of a token, never the token; a token idle for `tokens::IDLE_DAYS` expires. Every response
gets `nosniff`, `X-Frame-Options: DENY` and a CSP without inline scripts (`app::security_headers`)
— page JS goes into `static/js/`, never into a `<script>` block or an `on…=` attribute.

### Scanning

`scanner/run.rs` walks the books root and the music root as **two fully separate passes with
distinct `root` arguments**. `root` scopes the removal of vanished items; merging the passes or
swapping roots would remove the other library's items. An unreadable root is skipped entirely
rather than scanned as empty, for the same reason. Per-folder failures are collected and their
items left untouched.

The scan status is one `ScanStatus` per process in `AppState::scan` (legacy kept it in a module
singleton). `POST /scan` sets it and `tokio::spawn`s `run_scan` without awaiting, so
`/scan/status` can poll live progress. Each test builds its own `AppState`, so integration tests
don't share it — but **no two e2e specs may trigger a real scan concurrently**: they share one
server and the one `library_config` row.

### Library paths and data

The books and music roots are the single row in `library_config`, set at runtime under
Einstellungen → Bibliotheken, not via env. Env decides `DATABASE_URL`, `AUTOREVERSE_DATA`
(covers, artist images, playlist covers under `playlists/`, fetched artist pictures under `artists/info/`, station logos under
`radio/logos/`, downloaded episodes as `<Podcast>/<Folge>.<ext>`), the first admin,
`AUTOREVERSE_PODCAST_REFRESH_HOURS`, `AUTOREVERSE_LOUDNESS`/`_THREADS`, `AUTOREVERSE_ARTIST_INFO`/`_DAYS`, `AUTOREVERSE_DEVICES`, `ORIGIN`, `HOST`/`PORT`
and `AUTOREVERSE_STATIC` (else `static/` beside the binary, else `./static`; the server refuses
to start without it).

### Schema

`migrations/` holds sqlx migrations, embedded with `sqlx::migrate!` and applied at every start
(no switch). Never edit an applied migration — sqlx checksums them; add a new file. The schema
was started afresh for Rust: a database from the TS version is detected and refused with a German
hint instead of being migrated.

### Listening time and the "Entdecken" feed

`listen_events` is the one ledger behind `/me/stats` and the taste the suggestions build on
(`src/ai/`). Two things write to it: closing a session (`library::playback::close_session` — the
client's own `listened` when it sends one, else `position − start_position` for older clients)
and `POST /me/listens` (`book_listen`: time measured without a session, deduplicated per user by
the client's `key` in `client_key`, dated to `started_at`). Seconds are media seconds; every
claimed value passes `plausible_seconds`.

A row names its item always and its track where the client said which (`track_id`, nullable,
`ON DELETE SET NULL`): a close with `tracks: [{track_id, seconds}]` books **a row per track**,
all in one statement so they share their `at`, plus a row without a track for what `listened`
claims beyond them (`divide_session` is the whole rule). **Every reader of the ledger must
therefore add up rows, never count them**, when it means time or sessions: sum per item (the
feed, the Titel-Radio, the stats), fold consecutive rows (`listening_history`), or group by
`at` first where a session is to count once (`library::top`: plays of an album or an artist).
Track ids survive a rescan — `scanner::store` upserts tracks by path — and a renamed file's
rows move to the track that took its place (same item, same title); only then are the vanished
tracks deleted.

Closing a session without a `position` (`null` or absent) books `listened` and leaves `progress`
alone: that is how a single track out of a playlist, a mix or the radio is closed, so its album
neither shows up under "Weiter hören" nor loses its place. The web player marks such a session
`single` (`static/js/player/store.js`) and then sends no `PUT /progress` either. `GET /me/history`
(`listening_history`) reads the ledger back, folding consecutive events of one item into one
entry — per item, also where the rows name tracks.

`GET /radio` (`library::radio`) — "Titel-Radio" in every visible text, since "Radio" names the
internet stations — picks similar tracks for a track, an album or an artist without a
model: albums are scored against the seed (genre, artist, year) and the user's taste — the same
thresholds as `ai::music` (`MIN_SECONDS_FOR_TASTE`, `FAVORITE_SECONDS`) — plus chance, then deal
out a track per round under caps per album and artist. Two queries regardless of library size;
keep it that way. Chance comes in through `radio::Chance`, so the unit tests can seed it. The web
player queues the answer as single-track entries (`static/js/player/queue.js`) and, with
autoplay on (a `localStorage` switch under Einstellungen → Wiedergabe), plays on with a radio
when an album or a queue ends.

`GET /ai/feed` (`ai::feed::load`) mixes two kinds of lists. Those a model made are stored per
user in `ai_feed.payload` and regenerated in the background. Everything else — the album rails
and all spoken word (`new_episodes`, `unstarted_books`, `continue_series`, `rediscover_books`) —
is queried on every request and must never move into that payload: it has to be right the moment
an episode arrives or a book is begun. The feed therefore works without a model and without
music. Both contracts are spelled out for client authors in the README; keep it in step.

### Most played, the year in review

`library::top` behind `GET /me/top`, `/me/on-repeat`, `/me/repeat-rewind`, `/me/review` (API in
`api/top.rs`, pages `/library/stats` and `/library/review` in `web/stats.rs`). No model anywhere.
Rules: a play is a listen of ≥ 30 s (`PLAY_SECONDS`) — per row for a track, per `at` for an
album or artist; tracks/artists/genres are music only; missing items are left out; a fixed
number of statements per request. The queries aggregate `listen_events` by user and time before
they join anything (index `listen_event_user_at_cover`, which carries item, track and seconds);
keep new ones that way. `top_artists` is the exception that has to resolve a name per row — it
groups by a hash of the name and looks the name up only for the entries that make the list.

### Artist info

`library::artist_info` behind `GET /artists/{name}/info` and `…/pictures/{index}`: biography,
facts and pictures from TheAudioDB, the most played songs from Apple's catalogue, kept in
`artist_info`. Only names of the library are ever sent (`is_library_artist`: an album artist or a
track's own artist). `artist_info::job` fetches answers ahead of time for all of them, slowly
(`PACE`; both catalogues limit requests per minute), and the pictures of album artists; answers
older than `AUTOREVERSE_ARTIST_INFO_DAYS` are due again. A request never waits for a catalogue when
anything is kept — an outdated answer is served and renewed in a task (`Renewing` keeps it to one
per artist) — so keep any new catalogue call out of the request path.

### Lyrics

`library::lyrics` reads a track's lyrics from the collection and nowhere else: a `.lrc`/`.txt`
beside the audio file, else the tags (USLT/SYLT/TXXX, Vorbis comments, MP4 `©lyr`). **No lyrics
service is ever asked** — the owner has approved none. The text is not stored; `GET
/tracks/{id}/lyrics` reads the files on demand. Only `tracks.has_lyrics` is kept, by
`scanner::lyrics::refresh_flags` at the end of every scan, over all tracks: the files beside a
track are looked for every time (a `.lrc` added later changes no audio file, so its folder
counts as unchanged), the tags once per version of a file (`lyrics_embedded`, reset to NULL by
`scanner::store` when mtime or size change — NULL is also how tracks from before the column get
their flag on the next scan). A track whose folder or file cannot be seen keeps its flag. The
sidecar path is derived from the track's own path only and must resolve inside the track's
folder; anything over `MAX_LYRICS_BYTES` is not lyrics.

### Loudness

`scanner::loudness` gives every music track `gain_db` (towards `TARGET_LUFS` = −18),
`album_gain_db` and `peak`; `library::queries::Loudness` carries the three through every track
shape (`#[sqlx(flatten)]` / `#[serde(flatten)]` — a new track query selects the three columns).
The contract for client authors is in the README ("Loudness"); the apps are built against it.

- Tags win over measuring. `loudness::tags` reads them (ReplayGain in every container, Opus
  R128; not `iTunNORM`), once per version of a file: `read_pending_tags` at the end of every
  scan handles the tracks whose `loudness_state` is NULL, and `scanner::store` resets the state
  and the values when mtime or size change, and the state of a whole album when it loses or
  takes in a track (`forget_album_loudness`) — the album value is over all of its tracks.
  NULL is also how tracks from before the columns get theirs. A file that cannot be opened
  keeps its NULL.
- States: NULL → `pending` (music the tags left incomplete) or `done`; the pass turns
  `pending` into `done` or `failed`. `failed` is never retried until the file changes.
  Spoken word goes straight to `done`: it is not measured.
- Measuring (`loudness::measure`): `ffmpeg` decodes to float WAV on a pipe, the `ebur128`
  crate does R128 in-process — the album value is `loudness_global_multiple` over the states
  of all tracks, which is why the unit of work is the album and why an album with one open
  track is decoded whole. One transaction per album is what makes the pass resumable.
- The pass (`loudness::start`, `LoudnessJob` in `AppState`) runs after every scan and at
  server start, one at a time per process, `Config::loudness_threads` albums at once on
  blocking threads; `main` stops it on shutdown (the runtime would otherwise wait for the
  decoders). It never blocks the scan. A missing ffmpeg ends the pass without marking
  anything; an album with an unreachable file is postponed, not failed.
- The web player (`static/js/player/loudness.js`, applied in `store.js` `applyVolume`) sets
  `audio.volume`, so it only attenuates; album value for an album played as such, track value
  for `single` sessions, nothing for spoken word and stations. No second `<audio>` element:
  track changes are not gapless.

### Playlists: sharing, folders, export and import

`library::playlists`, `library::m3u`, `api/playlists.rs`, `web/playlists.rs`. The contract is in
the README ("Playlists", "Sharing a link"); the three apps are built against it, so it only ever
grows. Rules that must hold:

- **Who may do what is asked in one place**: `Playlist::visible_to` / `editable_by` /
  `is_owner`. API and web both go through their `find(…, Need::Read|Edit|Own)`: a playlist the
  user cannot see is 404 "Unbekannte Playlist" on every route (never 403 — that would tell that
  it exists), 403 only speaks of one the user sees. Admins are accounts like any other here.
  `collaborative` is stored as chosen but counts only while `shared`.
- `GET /playlists` without `scope` is the user's own playlists: old clients must not suddenly
  list other people's. New fields are additive; `PATCH` without any known field stays 422.
- A folder is a name on the playlist (`playlists.folder`), nothing else: no table, no nesting.
- The cover is the playlist's own upload under `Config::playlists_dir`, named by the id;
  `GET …/cover` is 404 without one (clients compose a mosaic), unlike an artist's picture.
- **No absolute path leaves the server**: an export goes through `paths::relative_to_roots`
  (music root, books root, the podcasts folder; else the bare file name).
- The import reads the collection once (`m3u::TrackIndex::load`) and answers every line from
  memory — one statement to match, one to insert, however long the file. Matching order: path
  (longest common suffix of folded components), then `#EXTINF` artist + title (`m3u::match_key`
  = accents folded + `musicbrainz::normalize`), length ±3 s as the tie-breaker. A wrong song is
  worse than a missing one: a match on the bare file name must be unambiguous.
- `/server/info` `web_url` is `ORIGIN` or `null`. Links are pages of the web UI and need a
  login; there are no public links. `?track=<id>` on the album page marks a row (`linked`).

### Internet radio stations

`stations/` and `api/stations.rs` (`/radio/stations…`), `web/stations.rs` (`/library/radio`).
Not `library::radio` — that is the Titel-Radio above. Rules that must hold:

- **Every address a station names is fetched through `stations::probe::client_for`**, i.e. the
  fenced client of `podcasts::net` (streams, playlist files, logos, a station's homepage and
  every icon that page names — when adding *and* on every play). The only exception is the host and port the operator configured as the directory when
  that is a literal local address: that is how the tests (one local server for directory and
  senders) and a LAN mirror work. Streams go through `stations::fetch::get`: a plain `http://`
  address whose answer the HTTP client cannot read is tried once more as SHOUTcast v1
  (`ICY 200 OK`, `stations::icy_v1` — one raw GET, no redirects), behind the same fence: it
  resolves only to public addresses unless `client_for` chose the unfenced client.
- **The logo route of the search hits takes an id, never an address**
  (`GET /radio/stations/search/logo?id=`, `stations::search_logos`, `SearchLogos` in
  `AppState::radio_logos`). The server remembers from the directory's answers which id has
  which logo and homepage, asks the directory once for an id it does not know, and keeps what
  it fetched — and failures, for a shorter time — in memory only, bounded by count and bytes.
  Requests for one id share a fetch (a `OnceCell` per id, run in a task of its own so a client
  hanging up does not undo it); a semaphore bounds the stations worked on at once.
- **The homepage is the fallback, never the first choice and never part of the search**
  (`stations::site_icon`): only when the directory's logo is missing or cannot be had is the
  page read (512 kB at most, by a small tag reader — there is no HTML parser in the project)
  and its icons tried in order (`candidates`: apple-touch-icon, icon by `sizes`, the two
  addresses browsers try unasked, tiny icons, `og:image`), `MAX_REQUESTS` requests and
  `Timing::site` seconds per page at most, one slot per homepage shared by every station that
  names it. `GET /radio/stations/search` fetches nothing of this; its `has_logo` therefore
  means "worth asking for" (`DirectoryStation::may_have_logo`), while a stored station's
  `has_logo` means the file is there. Stations added by address get their homepage's icon the
  same way (`SearchLogos::of_homepage`). README "Logos" and "Privacy" say so; keep them in
  step. The contract is in the README ("Logos");
  the apps are built against it.
- What counts as a logo is decided in one place, `stations::logos::normalize`, by content:
  JPEG/PNG/WebP/GIF as they come, `.ico` as the PNG of its largest picture, everything decoded
  once in full under size limits. **SVG is never passed on** (scripts, external references;
  there is no renderer). `logos::backfill` catches up on stored stations without a logo (from
  the directory, or with a homepage) when a list is read — in the background, each station
  once in a while (`SearchLogos::claim`); it is the one case where the directory is asked
  without a user searching or adding, and that does not happen with
  `AUTOREVERSE_RADIO_DIRECTORY=0` (the station's own homepage is still looked at).
- The stream route plays only rows of the caller's own list; the address comes out of the
  database, never out of the request. It must never become a proxy for arbitrary URLs — which
  is also why HLS is refused (`probe::HLS_UNSUPPORTED`) rather than rewritten.
- `live::open` holds the sender's connection and the user's slot (`Hub`, four streams per user)
  inside the response body's stream state, so a client hanging up frees both. Timeouts: ten
  seconds to the sender's headers, the client's read timeout between chunks, none on the whole.
- ICY metadata is requested from the sender, filtered out (`icy::IcyFilter`, a state machine
  that survives any chunk boundary) and kept per station in the `Hub` while somebody listens.
  `/now` never tunes in by itself.
- Radio is booked nowhere: no session, no progress, no `listen_events` (the ledger knows items
  and their tracks only). The web player's store plays a station as `current.live` (`playStation`), which
  switches off progress, sessions, seeking, the queue and autoplay.
- `AUTOREVERSE_RADIO=0` → every route 404 (`api::stations::RadioUser` checks before the login),
  no "Radio" in sidebar and chips (`Shell::radio`); `AUTOREVERSE_RADIO_DIRECTORY=0` → no
  request to the directory at all (homepages of stored stations are another matter, see
  above). `/server/info` reports both as `radio`, `radio_directory`.

### Remote control between devices

`devices/` (`Hub` in `AppState::devices`), `api/devices.rs` (`/me/devices…`), and in the web
player `static/js/player/devices.js` (logic, tested in `tests/js`), `devicelink.js`
(`EventSource`, reports, requests), `devices-ui.js`. The contract is in the README ("Remote
control"); the three apps are built against it, so it only ever grows. Rules that must hold:

- **The server only passes things on, and only in memory**: no table, no migration. A restart
  forgets every device; they reconnect and report again.
- One mutex (`Hub::accounts`), never held across an await. Everything sent to a stream is
  `try_send` into its bounded buffer; a full buffer ends that stream (`Account::overflowed` →
  `Hub::settle`), it never makes anybody wait.
- A device is looked up in the caller's own account only (`accounts[user.id]`). There is no path
  from a device id to another account — keep it that way, it is the whole isolation.
- Online = has a stream, or lost it less than `Timing::grace` ago (`Device::lost` holds the
  ticket of the timer that may remove it; a reconnect clears it). A stream that ends tells the
  hub through `Guard::drop`; stream numbers (`generation`) keep a replaced stream's end from
  counting as the device leaving.
- `devices` goes out when `State::differs_notably` from what was last sent (`Device::shown`),
  never for the position merely moving on. The JS has the same rule (`differsNotably`).
- Handing over moves the position on by the server's own clock since the report came in, not
  by the device's `at`. `at` goes out as UTC with milliseconds (`iso_millis`), deliberately not
  `serialize::to_iso`.
- Streams outlive the request that authenticated them: both logouts call `Hub::close_token`,
  and `api::devices::watch_token` looks the token up again every `Timing::recheck`. `main`
  calls `Hub::close_all` on shutdown, or a graceful shutdown would wait for the streams.
- `AUTOREVERSE_DEVICES=0` → every route 404 (`DevicesUser` checks before the login), no
  "Geräte" in the web player (`Shell::devices`), `/server/info` `devices: false`.
- Web player: all tabs of a browser share one device id, and exactly one tab holds the stream
  (`createElection`: the playing tab wins, agreed over a `BroadcastChannel`). A tab that lost
  must not reconnect by itself — that is the loop of two tabs throwing each other out.
- Tests shorten the waits through `Hub::new(Timing { … })` (`tests/integration/devices.rs`).

### Web UI

askama templates extend `templates/base.html` / `layouts/`; partials are rendered alone for
htmx swaps. htmx is vendored (`static/js/htmx-2.0.4.min.js`), nothing loads from a CDN. Page JS
lives in `static/js/` (player store and UI, metadata editors, infinite scroll, playlists). Styling
is plain CSS: design tokens in `static/css/tokens.css` (dark by default,
`:root[data-theme='light']` override, fixed accents `--music` / `--book` / `--podcast`), shared
classes in `app.css` (`.eyebrow`, `.mono`, `.pill`, `.badge`), components in `components.css`,
page styles in `css/pages/`. Prefer existing tokens and classes over new local values. Markup
(classes, ARIA roles, texts) mirrors legacy so the e2e specs and the design comparison hold.

## Testing layers

- Unit tests inline (`#[cfg(test)]`) for pure functions: tags, chapters, ranges, tokens,
  throttle, config, formatting.
- `tests/integration/` — one binary (`main.rs` + a module per area). `#[sqlx::test]` creates a
  throwaway database per test on the compose Postgres and applies the migrations; requests go
  through `TestApp` (`oneshot`). These port legacy's integration suites and assert exact German
  texts and wire shapes.
- `tests/js/` — `node --test`, for the browser modules in `static/js` that hold logic.
- `e2e/` — Playwright, only via `e2e/run-in-vm.sh`. Specs must not assume a clean database
  beyond what `e2e/seed.sql` sets up.

## Conventions

- **All user-facing strings are German**, including API `detail` texts and log lines meant for
  the operator; tests assert on the exact wording. Code, comments and identifiers are English.
- The JSON API stays wire-compatible with the iOS app: same paths and methods, snake_case,
  `{ "detail": … }` errors, timestamps through `serialize::to_iso` (whole seconds, `Z` — strict
  ISO-8601 clients reject `.000`). Build responses in `serialize.rs`, not by hand in a route.
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean. JS/CSS in `static/`
  uses tabs and single quotes.
- Commits: conventional commits in German with a scope (`fix(scanner): …`, `feat(podcasts): …`);
  the subject says what, the body why (decisions, deviations from legacy, what is left open).
