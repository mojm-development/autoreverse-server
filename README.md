# Autoreverse Server

A self-hosted media server for **audiobooks, music and podcasts** — with a JSON API for native
clients and a web UI you can use on its own.

Autoreverse scans your existing folders of audio files, reads their tags, extracts chapters and
cover art, and serves everything over HTTP with per-user playback progress, bookmarks, favorites
and playlists. Podcasts are subscribed by feed, browsable through a directory search, and
episodes can be downloaded to the server for offline access.

It is a single Rust binary backed by Postgres, with `ffprobe` as its only outside helper. No
transcoding daemon, no plugin system, no media-server sprawl — one container and a database.

> **Deutsche Fassung dieser Datei: [README.de.md](README.de.md).**
> Note that the **web UI is currently German-only**; the JSON API is language-neutral.

---

## Status

Early but real. The scanner, player, podcast subscriptions, user management and the full API
surface below all work. The server was rewritten from SvelteKit/TypeScript to Rust in version
`0.1.0`; the API stayed wire-compatible, the database did not (see
[Upgrading from the TypeScript version](#upgrading-from-the-typescript-version)). Run it against
data you have backups of.

---

## Features

**Library**

- Recursive scan of a books directory and a music directory, kept strictly separate
- Audio formats: `.mp3`, `.m4a`, `.m4b`, `.flac`, `.ogg`, `.opus`, `.wav`, `.aac`
- Tag-driven metadata: author, narrator, series and series index for books; artist, album artist
  and year for music
- Chapter extraction from `.m4b` files (via `ffprobe`, which reads the `chpl` atom that tag
  libraries miss)
- Cover art extracted from tags or picked up from the folder, cached server-side
- Items whose files have disappeared are removed from the database on the next scan, along with
  their progress, bookmarks and favorites. A library root that cannot be read is skipped whole,
  and a folder that fails to scan is left alone, so an unreachable mount never empties anything
- Scans are cancellable and report progress
- Metadata editing in the web UI, for single items and in bulk, with undo

**Playback**

- HTTP range streaming — seeking works, clients only fetch what they play
- Per-user playback position, resumed across devices, plus a "continue listening" list
- Bookmarks with timestamps, favorites for both whole items and single tracks
- Playlists with ordered entries
- Playback sessions, so you can see and close what is currently playing elsewhere

**Podcasts**

- Directory search (Apple's public iTunes search API) — search by name instead of hunting for
  feed URLs
- Preview a feed before subscribing
- Keep the newest N episodes of a subscription downloaded — a global default, overridable per
  feed. Downloads past that number are removed again on the next refresh
- Per-feed refresh, episodes downloaded to server-side storage on demand

**Users**

- Argon2id password hashing
- Bearer tokens for API clients, session cookies for the web UI, one resolver behind both
- Login throttling (`429` with `Retry-After`)
- Admin/non-admin split: user creation and library configuration are admin-only
- Per-user playback preferences

---

## Quick start (Docker Compose)

A ready-built image is published to GHCR as
[`ghcr.io/mojm-development/autoreverse-server`](https://ghcr.io/mojm-development/autoreverse-server)
(`latest` follows `main`; version tags follow releases). Save the following as `compose.yaml`,
adjust the two media mounts and the admin credentials, and you are done — the server applies its
database migrations on every start, no manual schema step needed. Images exist for `linux/amd64`
and `linux/arm64`:

```yaml
name: autoreverse

services:
  postgres:
    image: postgres:16
    restart: always
    environment:
      POSTGRES_USER: autoreverse
      POSTGRES_PASSWORD: autoreverse
      POSTGRES_DB: autoreverse
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ['CMD-SHELL', 'pg_isready -U autoreverse']
      interval: 2s
      timeout: 3s
      retries: 10

  app:
    image: ghcr.io/mojm-development/autoreverse-server:latest
    restart: always
    ports:
      - '8180:3000'
    environment:
      DATABASE_URL: postgresql://autoreverse:autoreverse@postgres:5432/autoreverse
      AUTOREVERSE_DATA: /data
      # First-admin bootstrap, only used once against an empty `users` table — change these.
      AUTOREVERSE_ADMIN_USER: admin
      AUTOREVERSE_ADMIN_PASSWORD: change-me
      # Required as soon as a reverse proxy sits in front — the public URL as the
      # browser sees it. Without it, logging in fails with "Cross-site POST form
      # submissions are forbidden". Drop it if you reach the server directly.
      ORIGIN: https://autoreverse.example.com
    volumes:
      # Point these at your own media; both are mounted read-only. After first login,
      # enter the in-container paths (/library/books, /library/music) under
      # Settings → Bibliotheken to actually use them.
      - /srv/media/audiobooks:/library/books:ro
      - /srv/media/music:/library/music:ro
      # Downloaded podcast episodes, reachable from the host. Writable for UID 1000
      # (the container's user), unlike the two mounts above. Drop this line to keep
      # them inside the volume.
      - /srv/media/podcasts:/data/podcasts
      - autoreverse-data:/data
    depends_on:
      postgres:
        condition: service_healthy

volumes:
  pgdata:
  autoreverse-data:
```

```sh
docker compose up -d
```

Alternatively, build from source — the repository ships a `compose.yaml` that builds the image
locally instead of pulling it:

```sh
git clone https://github.com/mojm-development/autoreverse-server.git
cd autoreverse-server

# Point these at your own media; both are mounted read-only.
export AUTOREVERSE_BOOKS_HOST=/srv/media/audiobooks
export AUTOREVERSE_MUSIC_HOST=/srv/media/music

# Optional: where downloaded podcast episodes should land on the host.
export AUTOREVERSE_PODCASTS_HOST=/srv/media/podcasts

docker compose up -d
```

Either way, the server is then on <http://localhost:8180>. The image has a built-in health check
(`autoreverse-server healthcheck`), so `docker compose ps` shows when it is ready. Log in with the
`AUTOREVERSE_ADMIN_USER` / `AUTOREVERSE_ADMIN_PASSWORD` from your `compose.yaml` — **change those
before the first start**, they create your admin account.

Then, still in the web UI:

1. **Settings → Bibliotheken**: enter the _in-container_ paths, `/library/books` and
   `/library/music`. Library roots live in the database, not in the environment (see below).
2. **Settings → Scan**: start the first scan.

---

## Configuration

| Variable                            | Required        | Default                                                           | Purpose                                                                                                         |
| ----------------------------------- | --------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`                      | yes             | `postgresql://autoreverse:autoreverse@localhost:5434/autoreverse` | Postgres connection string.                                                                                     |
| `AUTOREVERSE_DATA`                  | no              | `./data` (`/data` in the image)                                   | Base directory for server-managed files. `covers/`, `podcasts/` and `artists/` are created inside it.           |
| `AUTOREVERSE_ADMIN_USER`            | first boot only | —                                                                 | Username of the first admin account.                                                                            |
| `AUTOREVERSE_ADMIN_PASSWORD`        | first boot only | —                                                                 | Password of that account.                                                                                       |
| `AUTOREVERSE_PODCAST_REFRESH_HOURS` | no              | `1`                                                               | How often the server refreshes every feed and applies the download retention. `0` turns the background job off. |
| `ORIGIN`                            | behind a proxy  | derived from the `Host` header, assuming `http`                   | The public URL of the server. See [Running behind a reverse proxy](#running-behind-a-reverse-proxy).            |
| `HOST` / `PORT`                     | no              | `0.0.0.0` / `3000`                                                | Where the server listens.                                                                                       |
| `AUTOREVERSE_STATIC`                | no              | `static/` beside the binary, else `./static` (`/app/static` in the image) | CSS, JS and icons. The server refuses to start if it cannot find them.                                  |
| `RUST_LOG`                          | no              | `autoreverse_server=info,tower_http=warn`                         | Log filter.                                                                                                     |

The old `AUTOREVERSE_AUTO_MIGRATE` switch is gone: migrations are compiled into the binary and
always applied at start.

See [`.env.example`](.env.example) for a copy-paste starting point.

### Running behind a reverse proxy

If anything terminates TLS in front of Autoreverse — Caddy, nginx, Traefik — set `ORIGIN` to the
public URL, exactly as the browser sees it and without a trailing slash:

```yaml
environment:
  ORIGIN: https://autoreverse.example.com
```

Skip this and the web UI login fails with **`Cross-site POST form submissions are forbidden`**.
The reason: the server compares the browser's `Origin` header against its own origin on every
form POST. Behind a TLS-terminating proxy the browser reports `https://your.domain` while the
server, which only ever sees plain HTTP on port 3000, believes it is `http://<Host>`. The two
disagree, so the POST is rejected. Only form submissions are affected — the JSON API
authenticates fine, which is why the failure shows up at the login screen specifically. An
`https://` `ORIGIN` also marks the session cookie `Secure`.

`X-Forwarded-*` headers are not evaluated; `ORIGIN` is the only way to tell the server its public
URL.

### Getting at downloaded podcast episodes

Episodes downloaded through the UI are written to `$AUTOREVERSE_DATA/podcasts`, which lives inside
the container's data volume by default — fine for playback, awkward if you also want the files on
your NAS. Mount that one directory through to the host to change that:

```yaml
volumes:
  - /srv/media/podcasts:/data/podcasts # writable, unlike the library mounts
```

Two things to know before you do. The mount must be **writable** by the container's user, UID
1000 — the library mounts are `:ro`, this one cannot be. If the folder belongs to someone else on
the host, set `user: '<uid>:<gid>'` on the service. And the files are the server's own storage
that you are looking in on, not an export: one folder per podcast, named after the episode title
(`/data/podcasts/Some Show/Episode 42.mp3`), with characters that are unsafe on common file
systems replaced.

Cached cover art (`$AUTOREVERSE_DATA/covers`) stays in the volume; it is derived data and is
rebuilt on the next scan if you lose it.

### Library paths are not environment variables

The audiobook and music roots are stored **in the database** and configured at runtime under
_Settings → Bibliotheken_ (admin only). At deploy time your only job is making sure the paths you
intend to type there are actually mounted and readable by the server process. This is deliberate:
it lets an admin repoint a library without a redeploy.

### First-admin bootstrap

On every boot the server checks whether the `users` table is empty. If it is, **and** both
`AUTOREVERSE_ADMIN_USER` and `AUTOREVERSE_ADMIN_PASSWORD` are set, that user is created as the
first admin.

If the table is empty and the variables are unset, the server still starts but logs a warning and
nobody can log in. Set both before the very first boot against a fresh database. Leaving them set
afterwards is harmless; they are only read while the table is still empty.

A forgotten password, or an install without any admin, is fixed from the command line — the
password is asked for (or read from the first line of stdin), never passed as an argument:

```sh
docker compose exec app /app/autoreverse-server set-password admin --admin
```

This sets the password of an existing user (and signs out all of their sessions) or creates the
user. `--admin` makes them an admin.

### Upgrading from the TypeScript version

Version `0.1.0` replaces the SvelteKit server with a Rust one. The HTTP API, the web UI and the
file layout under `AUTOREVERSE_DATA` stay the same, but the database schema was started afresh
and there is **no automatic upgrade**. Pointed at a database of the old version, the server
refuses to start and says so. To move over:

1. Create a new, empty database (with the compose file above: `docker compose down -v` drops the
   old one together with everything in it) and point `DATABASE_URL` at it.
2. Start the server; the first admin comes from `AUTOREVERSE_ADMIN_USER`/`_PASSWORD` as before.
3. Enter the library paths again under _Settings → Bibliotheken_, scan, re-create the other users
   and re-subscribe podcasts. Progress, bookmarks, favorites and playlists do not carry over.

### External dependency

`ffprobe` (part of **ffmpeg**) must be on `PATH` for `.m4b` chapter extraction. The provided
Docker image installs it; on a bare-metal install, install ffmpeg yourself and keep the `static/`
directory next to the binary (or set `AUTOREVERSE_STATIC`). Without it, everything
else still works — `.m4b` files simply arrive without chapters.

---

## HTTP API

Authenticate once, then send the token as a bearer header:

```sh
TOKEN=$(curl -s -X POST http://localhost:8180/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"name":"admin","password":"change-me"}' | jq -r .token)

curl -s http://localhost:8180/items?kind=book -H "Authorization: Bearer $TOKEN"
```

Web-UI requests are authenticated by the `autoreverse_session` cookie instead; both paths resolve
to the same user. Errors come back as JSON with a German `detail` field, plus a `Retry-After`
header where a throttle applies. Timestamps are ISO-8601 in UTC with whole seconds
(`2026-10-14T18:22:57Z`).

`/progress/continue` returns `duration` (the summed track durations) and `has_cover` alongside
each entry, so a client can draw a progress bar without fetching the whole item.

`/items` takes `?sort=title|added|series|played` (default `title`); `played` orders by the asking
user's own progress. An unknown value is a 422 rather than a silent fallback.

| Method                 | Path                                              | Purpose                                                                |
| ---------------------- | ------------------------------------------------- | ---------------------------------------------------------------------- |
| `POST`                 | `/auth/login`                                     | Exchange credentials for a token                                       |
| `POST`                 | `/auth/logout`                                    | Revoke the current token                                               |
| `GET`                  | `/me`                                             | Current user                                                           |
| `GET`                  | `/me/stats`                                       | Listening time per day — `?days=` (default 30)                         |
| `POST`                 | `/me/password`                                    | Change own password                                                    |
| `GET` `PUT`            | `/me/playback`                                    | Playback preferences                                                   |
| `GET`                  | `/items`                                          | List items — `?kind=book\|album\|podcast\|episode`, `?q=`, `?missing=` |
| `GET` `PATCH`          | `/items/{id}`                                     | One item; edit its metadata (admin)                                    |
| `POST`                 | `/items/bulk`, `/items/bulk/undo`                 | Bulk metadata edit and its undo (admin)                                |
| `POST`                 | `/items/series`                                   | Detect or assign series and volume numbers, dry run by default (admin) |
| `GET`                  | `/items/{id}/children`                            | Tracks / episodes of an item                                           |
| `GET`                  | `/items/{id}/cover`                               | Cover image                                                            |
| `DELETE`               | `/items/missing`                                  | Purge items whose files are gone (admin)                               |
| `GET`                  | `/artists`                                        | Distinct artists                                                       |
| `GET`                  | `/library/more`                                   | Next page of the web library grid (used by the web UI)                 |
| `GET` `POST`           | `/artists/{name}/image`                           | Custom artist image; upload is admin, multipart `image`                |
| `PUT` `DELETE`         | `/artists/{name}/cover`                           | Pick an album as the artist image, or reset to random (admin)          |
| `GET`                  | `/search`                                         | Cross-library search                                                   |
| `GET` `PATCH`          | `/tracks/{id}`                                    | One track; edit its metadata (admin)                                   |
| `GET` `HEAD`           | `/tracks/{id}/stream`                             | Range-capable audio stream                                             |
| `POST`                 | `/play/{itemId}`                                  | Open a playback session                                                |
| `POST`                 | `/sessions/{sessionId}/close`                     | Close a playback session                                               |
| `GET` `PUT/POST`       | `/progress/{itemId}`, `/progress/continue`        | Playback progress and continue-listening                               |
| `GET` `POST` `DELETE`  | `/bookmarks`, `/bookmarks/{id}`                   | Bookmarks                                                              |
| `GET`                  | `/favorites`                                      | Favorites                                                              |
| `POST` `DELETE`        | `/favorites/items/{id}`, `/favorites/tracks/{id}` | Toggle favorites                                                       |
| `GET` `POST`           | `/playlists`                                      | List / create playlists                                                |
| `GET` `PATCH` `DELETE` | `/playlists/{id}`                                 | Read / rename / delete                                                 |
| `POST` `PUT` `DELETE`  | `/playlists/{id}/entries[/{entryId}]`             | Playlist entries                                                       |
| `GET`                  | `/podcasts/search`                                | Directory search by name                                               |
| `POST`                 | `/podcasts/preview`                               | Read a feed without subscribing                                        |
| `POST`                 | `/podcasts`                                       | Subscribe to a feed                                                    |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Refresh / unsubscribe                                                  |
| `POST`                 | `/episodes/{id}/download`                         | Download an episode to the server                                      |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Library scan control (admin)                                           |
| `GET` `POST`           | `/users`                                          | List / create users (admin)                                            |
| `PATCH` `DELETE`       | `/users/{id}`                                     | Update / delete a user (admin)                                         |
| `GET`                  | `/server/info`                                    | Library counts; user count for admins                                  |

---

## Development

You need a Rust toolchain (stable, edition 2024), Docker for Postgres, ffmpeg, and Node for the JS
tests. The `Makefile` wraps the usual steps (`make help`):

```sh
make setup            # .env from .env.example, dev-library/, Postgres on port 5434, first build
make dev              # cargo run on http://localhost:3000
```

Without make:

```sh
cp .env.example .env
docker compose up -d postgres   # Postgres on localhost:5434
cargo run                       # applies the migrations, creates the first admin, serves on :3000
```

Templates (askama) and migrations are compiled into the binary, so a template change needs a
restart; CSS and JS in `static/` are served from disk.

### Tests

```sh
cargo test                      # unit + integration tests
cargo test --test integration   # integration only: one throwaway database per test on :5434
node --test 'tests/js/*.test.mjs'  # browser-side JS modules
make test                       # fmt, clippy, cargo test, JS tests
```

The integration tests drive the real router in-process against Postgres; `.cargo/config.toml`
points them at the compose database, so `docker compose up -d postgres` is all they need.

The Playwright end-to-end suite lives in `e2e/`. It runs the browser in a macOS VM on a separate
build machine (`e2e/run-on-jarvis.sh`), not on the developer's desktop; adapt the script if you
want to run it elsewhere.

### Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

### Docker image

```sh
docker build -t autoreverse-server .
```

The `Dockerfile` cross-compiles in a `rust:*-slim` stage for the target architecture (no QEMU
for the Rust build) and ships the binary with `static/` on `debian:trixie-slim` plus ffmpeg,
running as UID 1000.

### Layout

```
src/
  api/          JSON endpoints, one module per area
  web/          server-rendered web UI (German), one module per area
  auth/         sessions, tokens, password hashing, login throttling, bootstrap
  library/      queries, progress, bookmarks, favorites, playlists, metadata editing
  podcasts/     feed parsing, directory search, episode downloads, retention, refresh job
  scanner/      book and music scans, tags, chapters, covers
  streaming/    HTTP range responses
  app.rs        the router; main.rs the command line
templates/      askama templates
static/         CSS, JS (htmx vendored), icons
migrations/     sqlx migrations
tests/          integration tests, JS tests, fixtures
e2e/            Playwright specs
```

---

## Contributing

Issues and pull requests are welcome.

- Run `make test` (or `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `node --test 'tests/js/*.test.mjs'`) before opening a PR.
- Schema changes go into a new file in `migrations/`; never edit one that has been released.
- The API is used by native clients: keep paths, field names (snake_case) and status codes
  compatible.
- The UI is German; please match the existing wording and tone in user-facing strings.

---

## License

Licensed under the **GNU Affero General Public License v3.0**. See [LICENSE](LICENSE).

The AGPL's network clause matters here: if you run a modified Autoreverse and let other people use
it over a network, you have to offer them the modified source as well.
