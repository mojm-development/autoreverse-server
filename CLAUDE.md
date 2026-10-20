# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Autoreverse Server: a self-hosted media server for audiobooks, music and podcasts. One Rust
binary (`autoreverse-server`: axum + tokio + sqlx/Postgres) serves both a JSON API for the native
clients (the iOS app) and a server-rendered German web UI (askama templates, htmx, small vanilla
JS modules). ffprobe/ffmpeg are external processes; nothing else runs beside it.

The code is a port of an earlier SvelteKit/TypeScript server, which in turn ported a Python/FastAPI
one. Comments call the TS version **"legacy"** (`legacy routes/items/+server.ts`, `legacy
scanState`) and sometimes cite review IDs (`C-2`, `I-6`, `Ruling 8`). Legacy is no longer in the
tree: read it with `git show 0b627bd:<path>` (0b627bd is the last commit before the rewrite; files
sit at the repo root there, not under `legacy/`). The JSON wire format is deliberately kept
byte-compatible with legacy — snake_case fields, German `detail` texts, status codes — except where
a comment or commit body marks an intentional divergence.

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

e2e/run-on-jarvis.sh [tests/login.e2e.ts]   # Playwright, browser in a Tart VM on Jarvis
docker build -t autoreverse-server .        # the release image
```

`.cargo/config.toml` points `DATABASE_URL` at the compose Postgres, so `cargo test` needs no
setup beyond `docker compose up -d postgres`; an exported `DATABASE_URL` wins. `ffprobe`/`ffmpeg`
must be on PATH (m4b chapters; `tests/integration/scan_api.rs` and `scanner_folders.rs` shell out
to `ffmpeg` to build fixtures).

**E2E never runs in a browser on this Mac.** `e2e/run-on-jarvis.sh` builds and starts the server
locally on :3100 against a fresh database `autoreverse_e2e` (seeded from `e2e/seed.sql`), boots the
Tart VM `autoreverse-e2e` on Jarvis, runs Playwright there and stops VM and server again. Only one
VM at a time on Jarvis; the script waits. `e2e/design/compare.sh` screenshots legacy and Rust side
by side the same way; it fetches legacy itself via `git archive 0b627bd` into a temp dir.

## Architecture

### Modules

```
src/
  main.rs       CLI dispatch (serve | set-password | healthcheck), migrations, first admin, serve
  app.rs        app(state) -> Router: the router factory, plus the cross-cutting middleware
  state.rs      AppState: pool, config, login throttle + semaphore, scan status, HTTP client
  config.rs     env → Config; find_static_dir
  db.rs         pool, embedded migrations, refuse a legacy (Drizzle) database
  error.rs      ApiError / WebError and their responses
  serialize.rs  wire format: rows in, snake_case JSON out, to_iso()
  api/          JSON routes, one file per area (items, tracks, playback, podcasts, users, …)
  web/          HTML routes, one file per area, plus layout, format helpers, error shell
  auth/         sessions/extractors, tokens, argon2 passwords, throttle, bootstrap
  library/      queries and mutations shared by api/ and web/ (progress, playlists, metadata …)
  scanner/      the scan: walk, tags (lofty), chapters (ffprobe), covers, store, status
  podcasts/     feeds (feed-rs), directory search, downloads, retention, the hourly job
  streaming/    HTTP Range responses
templates/      askama: base.html, layouts/, pages/, partials/ — compiled into the binary
static/         css/ (tokens.css, app.css, components.css, pages/*.css), js/, icons
migrations/     sqlx migrations — compiled into the binary, applied at every start
tests/          integration/ (one test binary, a module per area), js/, fixtures/
e2e/            Playwright specs, seed, run-on-jarvis.sh, design/ (parity tooling)
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
(covers, artist images, downloaded episodes as `<Podcast>/<Folge>.<ext>`), the first admin,
`AUTOREVERSE_PODCAST_REFRESH_HOURS`, `ORIGIN`, `HOST`/`PORT` and `AUTOREVERSE_STATIC` (else
`static/` beside the binary, else `./static`; the server refuses to start without it).

### Schema

`migrations/` holds sqlx migrations, embedded with `sqlx::migrate!` and applied at every start
(no switch). Never edit an applied migration — sqlx checksums them; add a new file. The schema
was started afresh for Rust: a database from the TS version is detected and refused with a German
hint instead of being migrated.

### Web UI

askama templates extend `templates/base.html` / `layouts/`; partials are rendered alone for
htmx swaps. htmx is vendored (`static/js/htmx-2.0.4.min.js`), nothing loads from a CDN. Page JS
lives in `static/js/` (player store and UI, metadata editors, infinite scroll, playlists). Styling
is plain CSS: design tokens in `static/css/tokens.css` (dark by default,
`:root[data-theme='light']` override, fixed accents `--music` / `--book` / `--podcast`), shared
classes in `app.css` (`.eyebrow`, `.mono`, `.pill`, `.badge`), components in `components.css`,
page styles in `css/pages/`. Prefer existing tokens and classes over new local values. Templates link
CSS and JS as `{{ crate::web::assets::prefix() }}/css/…` (`/v/<digest of static/>/…`), so a deploy
changes every asset URL and no browser or proxy cache keeps an old file. Markup
(classes, ARIA roles, texts) mirrors legacy so the e2e specs and the design comparison hold.

## Testing layers

- Unit tests inline (`#[cfg(test)]`) for pure functions: tags, chapters, ranges, tokens,
  throttle, config, formatting.
- `tests/integration/` — one binary (`main.rs` + a module per area). `#[sqlx::test]` creates a
  throwaway database per test on the compose Postgres and applies the migrations; requests go
  through `TestApp` (`oneshot`). These port legacy's integration suites and assert exact German
  texts and wire shapes.
- `tests/js/` — `node --test`, for the browser modules in `static/js` that hold logic.
- `e2e/` — Playwright, only via `e2e/run-on-jarvis.sh`. Specs must not assume a clean database
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
