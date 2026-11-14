# Autoreverse Server

A self-hosted media server for **audiobooks, music and podcasts** — with a JSON API for native
clients and a web UI you can use on its own.

Autoreverse scans your existing folders of audio files, reads their tags, extracts chapters and
cover art, and serves everything over HTTP with per-user playback progress, bookmarks, favorites
and playlists. Podcasts are subscribed by feed, browsable through a directory search, and
episodes can be downloaded to the server for offline access.

It is a single Rust binary backed by Postgres, with ffmpeg (`ffprobe`, `ffmpeg`) as its only outside helper. No
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
- Playlists with ordered entries, a description, a cover of their own and folders; shared with
  the other accounts of the server if you like, and open for their entries too; export and
  import as M3U
- Track radio ("Titel-Radio"): similar tracks from your own library for a track, an album or an artist — chosen from
  tags and your listening, without a language model or any outside service. The web player
  plays on with it when a list ends (Settings → Wiedergabe → Autoplay)
- Loudness normalisation: every music track gets a gain towards −18 LUFS, from its ReplayGain
  or R128 tags or measured by the server in the background; clients level the volume with it
  (web player: Settings → Wiedergabe → Lautstärke angleichen)
- Internet radio: public stations in a list per user, found in the free directory
  radio-browser.info or added by address, played through the server with the running title
- Remote control between your devices: see what plays where, steer it from another device,
  hand playback over ("Geräte" in the web player; the apps are built against the same routes)
- Playback sessions, so you can see and close what is currently playing elsewhere
- Listening statistics from what was actually heard, including time reported after playing offline
- Most played tracks, albums, artists and genres, "On Repeat" and "Repeat Rewind", and a review
  of each year — counted from your own listening, without a language model
- Lyrics from your own files: a `.lrc` or `.txt` beside a track or text embedded in its tags,
  timed lyrics running along in the web player. No lyrics service is asked

**Podcasts**

- Directory search (Apple's public iTunes search API) — search by name instead of hunting for
  feed URLs
- Preview a feed before subscribing
- Keep the newest N episodes of a subscription downloaded — a global default, overridable per
  feed. Downloads past that number are removed again on the next refresh
- Per-feed refresh, episodes downloaded to server-side storage on demand

**Users**

- Argon2id password hashing
- Bearer tokens for API clients, session cookies for the web UI, one resolver behind both; the
  database holds only a SHA-256 of each token, and a token unused for 180 days expires
- Login throttling (`429` with `Retry-After`), per name and per public client address
- Protection against cross-site requests (`Origin`, `Sec-Fetch-Site`), clickjacking protection and
  a Content-Security-Policy without inline scripts
- Files, artwork and chapters named by a feed on the internet are never fetched from the local
  network
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
| `AUTOREVERSE_DATA`                  | no              | `./data` (`/data` in the image)                                   | Base directory for server-managed files. `covers/`, `podcasts/`, `artists/` and `radio/` are created inside it.           |
| `AUTOREVERSE_ADMIN_USER`            | first boot only | —                                                                 | Username of the first admin account.                                                                            |
| `AUTOREVERSE_ADMIN_PASSWORD`        | first boot only | —                                                                 | Password of that account.                                                                                       |
| `AUTOREVERSE_PODCAST_REFRESH_HOURS` | no              | `1`                                                               | How often the server refreshes every feed and applies the download retention. `0` turns the background job off. |
| `ORIGIN`                            | behind a proxy  | derived from the `Host` header, assuming `http`                   | The public URL of the server. See [Running behind a reverse proxy](#running-behind-a-reverse-proxy).            |
| `AUTOREVERSE_TRUSTED_PROXIES`       | behind a proxy  | none                                      |
| `AUTOREVERSE_AI_PROVIDER`           | no              | off; `anthropic` or `ollama`              |
| `AUTOREVERSE_AI_API_KEY`            | for `anthropic` | —                                         |
| `AUTOREVERSE_AI_MODEL`              | for `ollama`    | `claude-haiku-5-5` for `anthropic`        |
| `AUTOREVERSE_AI_URL`                | no              | the provider's usual address              |
| `AUTOREVERSE_SCAN_HOURS`            | no              | `1`, `0` turns it off                     |
| `AUTOREVERSE_LOUDNESS`              | no              | on; `0` turns it off                      | Whether music without loudness tags is measured in the background (see [Loudness](#loudness)). Off: only tags are read. |
| `AUTOREVERSE_LOUDNESS_THREADS`      | no              | half the cores, at most 4                 | How many albums are decoded at once while measuring. |
| `AUTOREVERSE_ARTIST_INFO`           | no              | on; `0` turns it off                      | Whether the server asks TheAudioDB and Apple's music catalogue for an artist's biography, pictures and top tracks. Only the artist's name is sent. |
| `AUTOREVERSE_RADIO`                 | no              | on; `0` turns it off                      | Internet radio stations. Off: the `/radio/stations` routes answer 404 and the web UI has no "Radio". |
| `AUTOREVERSE_RADIO_DIRECTORY`       | no              | on; `0` turns it off                      | Whether stations can be searched in radio-browser.info. Off: the server does not ask the directory, stations are added by address only. Only search words, a country code and the ids of stations are sent there. |
| `AUTOREVERSE_RADIO_DIRECTORY_URL`   | no              | `https://all.api.radio-browser.info`      | The directory's base address: one name for all of its servers. Point it at a single server (`https://de1.api.radio-browser.info`) or a mirror if needed. |
| `AUTOREVERSE_DEVICES`               | no              | on; `0` turns it off                      | Remote control between the devices of an account (see [Remote control](#remote-control)). Off: the `/me/devices` routes answer 404 and the web player shows no "Geräte". |
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

`ORIGIN` is the only way to tell the server its public URL. `X-Forwarded-For` is read for one
purpose only: the login throttle. It counts attempts per client and name, so it needs to see
individual clients. List your proxy in `AUTOREVERSE_TRUSTED_PROXIES` (addresses or networks,
comma-separated) and make sure the proxy sets that header itself; from any other peer the header is
ignored.

Independently of `ORIGIN`, any state-changing request without a bearer token that the browser marks
as coming from another site (`Sec-Fetch-Site`) is refused. Every response carries
`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: same-origin` and a
Content-Security-Policy; a proxy does not need to add them. HSTS is the proxy's business.

The [remote control](#remote-control) holds one long-lived response open per device
(`GET /me/devices/events`, server-sent events). The server sends `X-Accel-Buffering: no`, which
keeps nginx from buffering it, and a line every 15 seconds, well within the usual read timeouts
(nginx `proxy_read_timeout`: 60 s). A proxy that buffers or compresses `text/event-stream`
regardless must be told not to for that path.

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

**ffmpeg** must be on `PATH`: `ffprobe` for `.m4b` chapter extraction, `ffmpeg` to decode music
whose loudness has to be measured. The provided
Docker image installs it; on a bare-metal install, install ffmpeg yourself and keep the `static/`
directory next to the binary (or set `AUTOREVERSE_STATIC`). Without it, everything
else still works — `.m4b` files simply arrive without chapters, and music without loudness tags
stays without a loudness value (the server says so in its log and tries again after the next
scan).

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
| `GET`                  | `/me/history`                                     | What was heard last — `?limit=`, see [Listening history](#listening-history) |
| `GET`                  | `/me/top`                                         | Most played tracks, albums, artists or genres, see [Most played and the year in review](#most-played-and-the-year-in-review) |
| `GET`                  | `/me/on-repeat`, `/me/repeat-rewind`              | Tracks played often lately; tracks played often once and not since     |
| `GET`                  | `/me/review`, `/me/review/years`                  | A calendar year of listening; the years there is one for               |
| `POST`                 | `/me/listens`                                     | Report listening time measured outside a session, see [Listening time](#listening-time) |
| `POST`                 | `/me/password`                                    | Change own password                                                    |
| `GET` `PUT`            | `/me/playback`                                    | Playback preferences                                                   |
| `GET`                  | `/items`                                          | List items — `?kind=book\|album\|podcast\|episode`, `?q=`, `?artist=`, `?genre=`, `?missing=` |
| `GET` `PATCH`          | `/items/{id}`                                     | One item; edit its metadata (admin)                                    |
| `POST`                 | `/items/bulk`, `/items/bulk/undo`                 | Bulk metadata edit and its undo (admin)                                |
| `POST`                 | `/items/series`                                   | Detect or assign series and volume numbers, dry run by default (admin) |
| `GET`                  | `/items/{id}/children`                            | Tracks / episodes of an item                                           |
| `GET`                  | `/items/{id}/cover`                               | Cover image                                                            |
| `DELETE`               | `/items/missing`                                  | Purge items whose files are gone (admin)                               |
| `GET`                  | `/artists`                                        | Distinct artists                                                       |
| `GET`                  | `/genres`                                         | Album genres with their album counts                                   |
| `GET`                  | `/library/more`                                   | Next page of the web library grid (used by the web UI)                 |
| `GET` `POST`           | `/artists/{name}/image`                           | Custom artist image; upload is admin, multipart `image`                |
| `PUT` `DELETE`         | `/artists/{name}/cover`                           | Pick an album as the artist image, or reset to random (admin)          |
| `GET`                  | `/artists/{name}/info?lang=`                      | Biography, facts, pictures and top tracks matched to the library       |
| `GET`                  | `/artists/{name}/pictures/{index}?size=`          | One of the pictures `info` lists, served from the server's disk        |
| `GET`                  | `/search`                                         | Cross-library search, including tracks by their artist and artists; tracks whose title holds the term come before those found by their artist |
| `GET`                  | `/radio`                                          | Similar tracks for a track, an album or an artist, see [Track radio](#track-radio) |
| `GET` `POST`           | `/radio/stations`, `/radio/stations/search`       | Internet radio stations of the user; directory search, see [Radio stations](#radio-stations) |
| `PATCH` `DELETE`       | `/radio/stations/{id}`                            | Rename, reorder, remove a station                                      |
| `GET` `HEAD`           | `/radio/stations/{id}/stream`, `…/now`, `…/logo`  | Live stream through the server, running title, logo                    |
| `GET`                  | `/radio/stations/search/logo?id=`                 | Logo of a search hit, fetched and served by the server                 |
| `GET` `PATCH`          | `/tracks/{id}`                                    | One track; edit its metadata (admin)                                   |
| `GET` `HEAD`           | `/tracks/{id}/stream`                             | Range-capable audio stream                                             |
| `GET`                  | `/tracks/{id}/lyrics`                             | The track's lyrics from the collection, see [Lyrics](#lyrics)          |
| `POST`                 | `/play/{itemId}`                                  | Open a playback session                                                |
| `POST`                 | `/sessions/{sessionId}/close`                     | Close a playback session: `{position?, listened?, tracks?}`, see [Listening time](#listening-time) |
| `GET` `PUT/POST`       | `/progress/{itemId}`, `/progress/continue`        | Playback progress and continue-listening                               |
| `GET` `POST` `DELETE`  | `/bookmarks`, `/bookmarks/{id}`                   | Bookmarks                                                              |
| `GET`                  | `/favorites`                                      | Favorites                                                              |
| `POST` `DELETE`        | `/favorites/items/{id}`, `/favorites/tracks/{id}` | Toggle favorites                                                       |
| `GET` `POST`           | `/playlists`                                      | List (`?scope=mine\|shared\|all`) / create playlists, see [Playlists](#playlists) |
| `GET` `PATCH` `DELETE` | `/playlists/{id}`                                 | Read / rename, describe, share / delete                                |
| `POST` `PUT` `DELETE`  | `/playlists/{id}/entries[/{entryId}]`             | Playlist entries                                                       |
| `GET` `PUT` `DELETE`   | `/playlists/{id}/cover`                           | The playlist's own cover                                               |
| `GET`                  | `/playlists/{id}/export`                          | The playlist as an M3U file (`?format=m3u8`) or as JSON (`?format=json`) |
| `POST`                 | `/playlists/import`                               | A new playlist from an M3U text                                        |
| `GET`                  | `/podcasts/search`                                | Directory search by name                                               |
| `GET`                  | `/podcasts`, `/podcasts/new`                      | Subscriptions with unheard count; unheard episodes across them              |
| `PUT`                  | `/podcasts/{id}/keep`                             | Episodes to keep: `{keep}`, `null` = default (admin)                        |
| `POST`                 | `/podcasts/preview`                               | Read a feed without subscribing (admin)                                |
| `POST`                 | `/podcasts`                                       | Subscribe to a feed (admin)                                            |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Refresh / unsubscribe (admin)                                          |
| `POST`                 | `/episodes/{id}/download`                         | Download an episode to the server                                      |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Library scan control (admin); `loudness` reports the measuring pass    |
| `GET` `POST`           | `/users`                                          | List / create users (admin)                                            |
| `PATCH` `DELETE`       | `/users/{id}`                                     | Update / delete a user (admin)                                         |
| `GET` `DELETE`         | `/me/devices`, `/me/devices/events`, `/me/devices/{device_id}` | The account's online devices, a device's event stream, signing off — see [Remote control](#remote-control) |
| `PUT` `POST`           | `/me/devices/{device_id}/state`, `…/commands`, `…/transfer` | Report what plays, steer another device, hand playback over |
| `GET`                  | `/server/info`                                    | Library counts; user count for admins; `radio`, `radio_directory`, `devices`; `web_url`, see [Sharing a link](#sharing-a-link) |
| `GET`                  | `/ai/status`                                      | Whether a language model is set up                                     |
| `GET` `POST`           | `/ai/feed`, `/ai/feed/refresh`                    | The "Entdecken" feed, see [Discover feed](#discover-feed); generate its model-made parts anew |
| `POST`                 | `/ai/playlist`, `/ai/suggestions`, `/ai/discover` | A playlist from a sentence (`{prompt}`), album suggestions, new artists — need a model |

### Listening time

`/me/stats` and the suggestions on "Entdecken" both read one ledger of listening time. Time gets
into it in two ways. Use exactly one of them for any stretch of playback, or it counts twice.

All times are **media seconds**: how much of the recording was audible, not how long it took. An
hour of a book heard at double speed is 3600, not 1800. Count the time the player actually
advanced while playing — not the distance between two positions, which is wrong after any seek.

**With a session** (`POST /play/{itemId}` … `POST /sessions/{sessionId}/close`). The close body:

| Field      | Type            | Required | Meaning                                                              |
| ---------- | --------------- | -------- | -------------------------------------------------------------------- |
| `position` | number or null  | no       | Where the item stands now, in seconds along its tracks. Saved as progress. `null` or absent: the progress is left alone, see below. |
| `listened` | number or null  | no       | Media seconds heard during this session, measured by the client.     |
| `tracks`   | array or null   | no       | How that time divides among the item's tracks: `[{"track_id": integer, "seconds": number}]`. Lets the statistics know which songs were heard, see below. |

```json
{ "position": 5400.5, "listened": 312.25 }
```

```json
{ "position": 812.4, "listened": 640,
  "tracks": [ { "track_id": 9051, "seconds": 602.5 }, { "track_id": 9052, "seconds": 37.5 } ] }
```

```json
{ "position": null, "listened": 214 }
```

- With `listened`, exactly that is booked, cut to what is plausible: not below 0, not above
  (seconds since the session was opened + 60) × 4 (the fastest playback speed), not above 7 days.
  A value cut down is still a success. `0` books nothing.
- Without `listened` (or `null`), the server books `position − start_position` as before, and
  nothing if that is negative. This is what clients from before the field get. It overcounts a
  jump ahead and loses a jump back, so new clients should always send `listened`.
- **Without `position`** (`null` or absent) the session is closed and `listened` is booked, but
  the item's saved progress stays exactly as it is: no entry is created, none is overwritten,
  its `finished` flag and its place under `/progress/continue` do not change. Send this after a
  **single track** out of a playlist, a mix or the radio — one song must not put its album under
  "continue listening" or move where the album was left. Such a client must not send
  `PUT /progress/{itemId}` for that session either. With neither `position` nor `listened`
  the session is closed and nothing is booked (the fallback above needs a position).
- **With `tracks`** the time is booked per track, which is what `/me/top`, `/me/on-repeat` and
  the review's top tracks are made of. Each entry is the media seconds heard of one track of
  **this session's item** during this session; a track that was not heard is left out, a track
  heard in two stretches may appear once with the sum or twice. One rule decides what is booked,
  whatever else the body holds:
  - The session's total is the **larger** of `listened` and the sum of the entries, cut to what
    is plausible exactly as `listened` alone is (see above).
  - Every entry is booked to its track. If the sum of the entries alone is above that bound,
    all of them are shortened by the same factor so that together they are the bound.
  - What is left of the total after the entries — `listened` was larger than their sum — is
    booked to the item **without a track**. If `listened` is smaller than the sum, or absent,
    nothing is left over and the sum counts.

  So send `listened` as before and `tracks` as its breakdown; they normally add up to the same.
  An entry is dropped without an error when its `track_id` is not a track of the session's item
  (another album, unknown, deleted meanwhile), when `track_id` is not a whole number or when
  `seconds` is not a number above 0. If no entry is left — or `tracks` is absent, `null` or
  `[]` — everything is exactly as without the field, including the `position − start_position`
  fallback. `tracks` that is not a list is `422` "tracks muss eine Liste sein", more than 5000
  entries `422` "Höchstens 5000 Einträge in tracks"; the session stays open. The item's total in
  `/me/stats`, `/me/history` and the suggestions is the same with and without `tracks`: a
  session of three tracks is three rows of the ledger, and they add up to the one session.
- `listened` that is not a number is `422` `{"detail": "listened muss eine Zahl sein"}`, a
  `position` that is there but not a number (a string, a list) is `422` "position muss eine Zahl
  sein"; the session stays open. Other answers: `204` on success, `404` "Unbekannte Sitzung"
  for an unknown session, one of another user or one closed before. Unknown fields are ignored.

Opening a session (`POST /play/{itemId}`) saves nothing about progress: it reads the saved
position (handing it out as `start_position`, `0` for a finished item) and records the session
itself. No progress entry is made, "continue listening" and `sort=played` are untouched until a
`PUT /progress/{itemId}` or a close **with** a position.

**Without a session** — media played offline and handed in later, single tracks, playlists:
`POST /me/listens`.

```json
{
  "listens": [
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a11", "item_id": 412, "seconds": 1830.5, "started_at": "2026-11-10T06:12:00Z" },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a12", "track_id": 9051, "seconds": 214, "started_at": "2026-11-12T17:40:03+01:00" }
  ]
}
```

| Field        | Type    | Required        | Meaning                                                                    |
| ------------ | ------- | --------------- | -------------------------------------------------------------------------- |
| `key`        | string  | yes             | The client's own name for this entry, 1–100 characters, e.g. a UUID made when the entry is recorded. The same key is booked once per user, however often it is sent. |
| `item_id`    | integer | one of the two  | The book, album or episode.                                                |
| `track_id`   | integer | one of the two  | A single track. The time counts for the item it belongs to and is booked to the track as well, for the [statistics per track](#most-played-and-the-year-in-review). If both are sent, the track must belong to that item. |
| `seconds`    | number  | yes             | Media seconds heard, greater than 0.                                       |
| `started_at` | string  | yes             | When this stretch of listening began, ISO-8601 with an offset or `Z`; fractions of a second are fine. |

At most 500 entries per request. Limits per entry: `started_at` may lie up to 365 days back and
no more than 5 minutes ahead of the server's clock (for a client whose clock runs fast); `seconds`
is cut to (seconds since `started_at` + 300) × 4 and to 24 hours. The time is counted on the UTC
day of `started_at`, so what was heard offline last week shows up in last week's statistics.

The answer is `200` with one result per entry, in the order sent:

```json
{
  "results": [
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a11", "status": "booked", "seconds": 1830.5 },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a12", "status": "duplicate" },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a13", "status": "rejected", "detail": "Unbekanntes Item" }
  ]
}
```

| `status`    | Meaning                                                                                  |
| ----------- | ---------------------------------------------------------------------------------------- |
| `booked`    | Counted. `seconds` is what was booked — fewer than sent if it was cut.                   |
| `duplicate` | An entry with this key was booked before; nothing changed.                               |
| `rejected`  | Not bookable; `detail` says why in German (unknown item or track, a missing or malformed field, `started_at` in the future or too old). `key` is `null` if the entry had no usable one. |

Every status is final: remove the entry from the local queue whichever comes back, and keep it
only if the request as a whole failed (no connection, `401`, `5xx`). The request as a whole is
`422` only when the body is not `{"listens": [...]}` ("listens muss eine Liste sein") or holds
more than 500 entries. `/me/listens` does not touch the playback position; save that with
`PUT /progress/{itemId}` as before.

### Listening history

`GET /me/history?limit=` lists what the asking user heard last, newest first. `limit` is 1–200,
default 50; anything else is `422` "limit außerhalb des gültigen Bereichs".

```json
{
  "entries": [
    {
      "listened_at": "2026-11-12T17:43:37Z",
      "seconds": 2512.5,
      "item": {
        "id": 412, "kind": "album", "title": "Neu! 75", "has_cover": true,
        "author": null, "artist": "Neu!", "series": null, "series_index": null,
        "year": 1975, "missing_since": null, "published_at": null
      }
    }
  ]
}
```

| Field         | Type   | Content                                                                        |
| ------------- | ------ | ------------------------------------------------------------------------------ |
| `listened_at` | string | When the latest part of this entry was heard, ISO-8601 in UTC, whole seconds.  |
| `seconds`     | number | Media seconds heard in this entry, summed.                                     |
| `item`        | object | The book, album or episode, in the shape `/items` lists (`ItemSummary`).       |

The source is the ledger of [listening time](#listening-time), so an entry appears once a
session is closed or a report is handed in — not while something is still playing. Bookings of
the same item that follow one another are **one** entry (an album heard over three sessions is
one line: seconds added, the latest time); the same item appears again if something else was
heard in between. Items whose files are gone are left out, and what was heard either side of one
joins up. The history is **per item, not per track**: a session booked track by track (`tracks`
on closing it) is still one entry for its album, and so is what `/me/listens` reports for a
`track_id`. An empty history is `{"entries": []}`. The history reaches back over the newest 5000
bookings.

### Most played and the year in review

Five routes read the ledger of [listening time](#listening-time) back as statistics. None of
them needs a language model; all are per user and answer `401` without a login.

**What counts.** Seconds are media seconds, as everywhere. A **play** ("Hörvorgang") is a listen
of at least 30 seconds: for a track, one booking of that track (one row per track and session, or
one `/me/listens` entry); for an album or an artist, the bookings of one moment added up — so an
album heard in one session is **one** play however many of its tracks ran, and a session of
three songs is not three plays of the album. Tracks, artists and genres count **music only**
(albums); time heard in books and episodes appears in the review's totals and nowhere in the top
lists. Listening booked without a track — everything from before clients sent `tracks`, and
`/me/listens` entries with only an `item_id` — counts for albums, artists and genres but cannot
appear under tracks. Items whose files are gone (`missing_since`) are left out of every list.

#### `GET /me/top`

| Parameter | Type    | Meaning                                                                         |
| --------- | ------- | ------------------------------------------------------------------------------- |
| `kind`    | string  | Required: `tracks`, `albums`, `artists` or `genres`.                            |
| `period`  | string  | `week` (the last 7 days), `month` (30 days, the default), `year` (365 days) or `all`. Sliding, counted back from now. |
| `year`    | integer | A calendar year in UTC, 2000–2100, e.g. `2026`. If present it is used **instead of** `period`. |
| `limit`   | integer | 1–100, default 25.                                                              |

```json
{
  "kind": "tracks",
  "from": "2026-10-14T09:30:00Z",
  "to": "2026-11-13T09:30:00Z",
  "entries": [
    {
      "track": { "id": 9051, "title": "Hallogallo", "artist": null, "duration": 602.5,
                 "item_id": 412, "item_title": "Neu!", "item_kind": "album", "has_lyrics": false },
      "seconds": 4217.5,
      "plays": 7
    }
  ]
}
```

| Field     | Type           | Content                                                                         |
| --------- | -------------- | ------------------------------------------------------------------------------- |
| `kind`    | string         | The `kind` asked for.                                                           |
| `from`    | string or null | Start of the period, inclusive. `null` for `period=all`.                        |
| `to`      | string         | End of the period, **exclusive**: the moment of the request, or for `year=` the first second of the following year (`2027-01-01T00:00:00Z`), also while that year is still running. |
| `entries` | array          | Longest listened first (`seconds` descending, then `plays`). Its shape depends on `kind`, see below. Empty when nothing was heard. |

| `kind`    | One entry                                                                                   |
| --------- | ------------------------------------------------------------------------------------------- |
| `tracks`  | `{"track": TrackSummary, "seconds": number, "plays": integer}` — `TrackSummary` as in [Track radio](#track-radio). |
| `albums`  | `{"item": ItemSummary, "seconds": number, "plays": integer}` — `ItemSummary` as `/items` lists it; always `kind: "album"`. |
| `artists` | `{"artist": string, "seconds": number, "plays": integer, "has_image": boolean}`             |
| `genres`  | `{"genre": string, "seconds": number}` — no `plays`.                                        |

```json
{ "item": { "id": 412, "kind": "album", "title": "Neu!", "has_cover": true, "author": null,
            "artist": "Neu!", "series": null, "series_index": null, "year": 1972,
            "missing_since": null, "published_at": null },
  "seconds": 9120, "plays": 4 }
```

```json
{ "artist": "Neu!", "seconds": 15230.5, "plays": 11, "has_image": true }
```

```json
{ "genre": "Krautrock", "seconds": 20110 }
```

- **Artists.** A booking counts for the track's own artist where it names a track that has one
  (a sampler's songs, a guest), else for the album's artist. Spellings that differ only in case
  are one artist. `has_image` says whether `GET /artists/{artist}/image` answers with a picture
  for exactly this `artist` string (URL-encode it) — true for album artists with artwork or an
  uploaded image, false for most track-only artists.
- **Genres** are the album's genre; albums without one are left out. Case-insensitive.

| Status | `detail`                                               | When                                    |
| ------ | ------------------------------------------------------ | --------------------------------------- |
| `422`  | "kind muss tracks, albums, artists oder genres sein"   | `kind` missing or something else.       |
| `422`  | "period muss week, month, year oder all sein"          | An unknown `period` (and no `year`).    |
| `422`  | "year außerhalb des gültigen Bereichs"                 | `year` not a whole number in 2000–2100. |
| `422`  | "limit außerhalb des gültigen Bereichs"                | `limit` outside 1–100.                  |

#### `GET /me/on-repeat` and `GET /me/repeat-rewind`

Both take `?limit=` (1–100, default 30; else `422` "limit außerhalb des gültigen Bereichs") and
answer with tracks in the shape of `GET /radio` — play them the same way, as single tracks:

```json
{ "tracks": [ { "id": 9051, "title": "Hallogallo", "artist": null, "duration": 602.5,
                "item_id": 412, "item_title": "Neu!", "item_kind": "album", "has_lyrics": false } ] }
```

- **`/me/on-repeat`**: the tracks with at least **2 plays in the last 30 days**, most plays
  first (ties: more seconds first).
- **`/me/repeat-rewind`**: the tracks with at least 2 plays **more than 30 days ago** that were
  not heard at all in the last 30 days — not even for a few seconds — most plays first.

Music only, nothing from missing albums, no track twice. **An empty list is a valid answer**
(`{"tracks": []}`): a new user, or one whose clients do not report tracks yet.

#### `GET /me/review` and `GET /me/review/years`

`GET /me/review?year=2026` is one **calendar year in UTC**; without `year` it is the current
year. A year in which nothing was heard is not a `404`: it comes back with zeros, `null`s and
empty lists, exactly in the shape below. `year` outside 2000–2100 is `422` "year außerhalb des
gültigen Bereichs".

```json
{
  "year": 2026,
  "seconds": { "total": 612340.5, "music": 401200, "books": 180040.5, "podcasts": 31100 },
  "days_listened": 241,
  "longest_day": { "date": "2026-03-14", "seconds": 21840 },
  "longest_streak_days": 37,
  "months": [41200, 38010.5, 60300, 52100, 49800, 47020, 55110, 61000, 58800, 70500, 78500, 0],
  "top_tracks": [ { "track": { "id": 9051, "title": "Hallogallo", "artist": null, "duration": 602.5, "item_id": 412, "item_title": "Neu!", "item_kind": "album", "has_lyrics": false }, "seconds": 4217.5, "plays": 7 } ],
  "top_albums": [ { "item": { "id": 412, "kind": "album", "title": "Neu!", "has_cover": true, "author": null, "artist": "Neu!", "series": null, "series_index": null, "year": 1972, "missing_since": null, "published_at": null }, "seconds": 9120, "plays": 4 } ],
  "top_artists": [ { "artist": "Neu!", "seconds": 15230.5, "plays": 11, "has_image": true } ],
  "top_genres": [ { "genre": "Krautrock", "seconds": 20110 } ],
  "books_finished": [ { "id": 97, "kind": "book", "title": "Tehanu", "has_cover": true, "author": "Ursula K. Le Guin", "artist": null, "series": "Erdsee", "series_index": 4, "year": 1990, "missing_since": null, "published_at": null } ],
  "new_artists": 23,
  "first_listen": { "at": "2026-01-01T08:12:40Z", "item": { "id": 412, "kind": "album", "title": "Neu!", "has_cover": true, "author": null, "artist": "Neu!", "series": null, "series_index": null, "year": 1972, "missing_since": null, "published_at": null } }
}
```

| Field                 | Type            | Content                                                             |
| --------------------- | --------------- | ------------------------------------------------------------------- |
| `year`                | integer         | The year this is about.                                             |
| `seconds`             | object          | `total`, `music` (albums), `books`, `podcasts` (episodes) — numbers; `total` is the sum of the three. Includes time heard in items that have gone missing since. |
| `days_listened`       | integer         | UTC days with any listening.                                        |
| `longest_day`         | object or null  | `{"date": "YYYY-MM-DD", "seconds": number}` — the UTC day with the most listening, the earliest of equals. `null` in an empty year. |
| `longest_streak_days` | integer         | The most consecutive UTC days with any listening, within this year. |
| `months`              | array           | Always 12 numbers: seconds per month, January first.                |
| `top_tracks`, `top_albums`, `top_artists`, `top_genres` | arrays | Up to 5 entries each, exactly the entries of `/me/top?kind=…&year=…`. |
| `books_finished`      | array           | `ItemSummary` of every book finished in this year, in the order they were finished. The date is when it was marked finished; saving it as finished again does not move it. A book begun again since is not finished and not listed. (Books finished before this was kept count by the last time their position was saved.) |
| `new_artists`         | integer         | Artists (as in `/me/top?kind=artists`) whose first listen ever falls into this year. |
| `first_listen`        | object or null  | `{"at": ISO timestamp, "item": ItemSummary}` — the first thing heard in the year; any kind of item. `null` in an empty year. |

`GET /me/review/years` lists the calendar years (UTC) that have any listening, newest first —
what a year picker offers. `{"years": []}` for a new user.

```json
{ "years": [2026, 2025] }
```

### Lyrics

Lyrics come **out of the collection itself** — the server asks no lyrics service and sends no
track name anywhere. Sources, the first that has a text wins:

1. A file beside the audio file with the same name and the ending `.lrc` (timed) or `.txt`
   (plain); `.lrc` goes before `.txt`. `01 Hallogallo.flac` → `01 Hallogallo.lrc`.
2. Text embedded in the audio file: ID3v2 `USLT` (plain) and `SYLT` (timed) frames and free-form
   `TXXX` frames named `LYRICS`, `UNSYNCEDLYRICS`, `SYNCEDLYRICS` or `lyrics-<language>` (`SYLT`
   and `TXXX` in MP3 files); the
   Vorbis comments `LYRICS`, `UNSYNCEDLYRICS`, `SYNCEDLYRICS` (FLAC, Ogg, Opus); MP4 `©lyr`.
   Embedded text that itself carries LRC time stamps counts as timed, and timed text goes before
   plain text.

`GET /tracks/{id}/lyrics`:

```json
{
  "synced": true,
  "source": "file",
  "lines": [
    { "t": 12.5, "text": "Erste Zeile" },
    { "t": 17.02, "text": "Zweite Zeile" },
    { "t": 24, "text": "" },
    { "t": 31.4, "text": "Neue Strophe" }
  ]
}
```

```json
{
  "synced": false,
  "source": "tags",
  "lines": [
    { "t": null, "text": "Erste Zeile" },
    { "t": null, "text": "Zweite Zeile" },
    { "t": null, "text": "" },
    { "t": null, "text": "Neue Strophe" }
  ]
}
```

| Field    | Type           | Content                                                                         |
| -------- | -------------- | ------------------------------------------------------------------------------- |
| `synced` | boolean        | `true`: every line has a time. `false`: no line has one.                        |
| `source` | string         | `file` (a `.lrc`/`.txt` beside the track) or `tags` (embedded).                 |
| `lines`  | array          | In reading order; for synced text in playing order (`t` ascending). Never empty. |
| `t`      | number or null | Seconds from the start of **the track** (not of the album) at which the line begins; fractions are common. `null` in unsynced text. |
| `text`   | string         | One line, without a line break. `""` is a line too: a gap between verses, or in synced text an instrumental stretch — keep it. |

In synced text the current line is the last one whose `t` is ≤ the position within the track;
before the first `t` no line is current. Several lines can share a `t` (a line and its
translation). What the server does with LRC: a line with several stamps (`[00:12.00][01:15.00]`)
becomes one line per stamp; header lines (`[ar:…]`, `[ti:…]`, `[length:…]`) are dropped;
`[offset:±ms]` is applied (positive: earlier), never below 0; word stamps of "enhanced" LRC
(`<00:12.34>`) are removed; text without a stamp gets the time of the line before it. Stamps are
`[mm:ss]`, `[mm:ss.x]` to `[mm:ss.xxx]`, with `.`, `:` or `,` before the fraction.

| Status | `detail`                  | When                                                         |
| ------ | ------------------------- | ------------------------------------------------------------ |
| `404`  | "Kein Songtext vorhanden" | The track exists and has no lyrics.                          |
| `404`  | "Unbekannter Track"       | No such track.                                               |

Limits: a text of more than 200 kB (file or embedded) is not treated as lyrics. Text files are
read as UTF-8 (with or without a byte order mark), UTF-16 with a byte order mark, or else
Latin-1; tags in whatever encoding their frame declares. Only the file beside the track is ever
read — a link that leads out of the track's folder is ignored.

**`has_lyrics`.** Every track shape — the `tracks` of `POST /play/{itemId}` and of
`GET /items/{id}`, `GET /tracks/{id}`, and every `TrackSummary` (search, favorites, playlists,
radio, the lists above), as well as the `tracks` of `POST /ai/playlist` and of the
`mixes` in `GET /ai/feed` — carries `has_lyrics: boolean`, so a client shows its lyrics button
only where it leads somewhere. The flag is kept by the library scan: every scan (the automatic
one every `AUTOREVERSE_SCAN_HOURS`, or one started by hand) looks for the files beside every
track, also in folders it otherwise skips as unchanged, and reads a file's tags for lyrics once
per version of the file. A library from before this feature therefore gets its flags with the
first scan after the update, without anybody doing anything; that scan reads the tags of every
track once and takes correspondingly longer. Between scans the flag can lag behind the files —
`GET /tracks/{id}/lyrics` always reads the files of that moment and corrects the flag of its
track when the two disagree. Clients that do not know the field ignore it.

### Loudness

"Lautstärke angleichen": the server knows how loud each music track is, the client turns the
volume. Every track shape — the `tracks` of `POST /play/{itemId}` and of `GET /items/{id}`,
`GET /tracks/{id}`, and every `TrackSummary` (search, favorites, playlists, track radio, On
Repeat, the top lists) — carries three more fields. Each is `null` until a value is known;
clients that do not know them ignore them.

| Field           | Type           | Content                                                                 |
| --------------- | -------------- | ----------------------------------------------------------------------- |
| `gain_db`       | number or null | dB to turn the track by to reach **−18 LUFS** (the ReplayGain 2 reference). Negative: turn down. |
| `album_gain_db` | number or null | The same for the album as a whole — one value on all of its tracks, so that quiet and loud pieces keep their relation. |
| `peak`          | number or null | The track's highest sample, linear (`1.0` = full scale; lossy files can be slightly above). |

```json
{ "id": 9051, "position": 1, "title": "Hallogallo", "duration": 602.5, "artist": null, "disc": 1,
  "has_lyrics": false, "gain_db": -6.48, "album_gain_db": -5.1, "peak": 0.988553 }
```

**Applying it.** Multiply the samples by `10^(gain/20)`. Use `album_gain_db` while an album
plays as such (no step between its tracks) and `gain_db` for a track on its own (playlist,
queue, radio); where only one of the two is known, take that one. A positive gain is a boost:
limit the factor so that `factor × peak ≤ 1`, and do not boost at all without a `peak`. Leave
spoken word and radio stations alone.

**Where the values come from**, in this order:

1. Tags already in the file, read during the scan: `REPLAYGAIN_TRACK_GAIN`,
   `REPLAYGAIN_ALBUM_GAIN`, `REPLAYGAIN_TRACK_PEAK` (Vorbis comments in FLAC/Ogg, ID3 `TXXX`,
   APEv2 in MP3, MP4 `----:com.apple.iTunes:…`), with `REPLAYGAIN_REFERENCE_LOUDNESS` honoured
   when it names another reference; Opus `R128_TRACK_GAIN`/`R128_ALBUM_GAIN` (Q7.8, towards
   −23 LUFS, converted by +5 dB). `iTunNORM` is not read: it cannot be interpreted reliably.
2. Measured by the server where the tags leave something open: EBU R128 integrated loudness
   and the sample peak, by decoding the file once with `ffmpeg`. The album value is the gated
   loudness over all tracks of the album together, not the mean of their values.

Only **music** is measured. Audiobooks and podcast episodes keep `null` unless their files
carry tags — decoding hundreds of hours of speech would buy nothing.

**The measuring pass** runs in the background after every scan and at every start, album by
album, `AUTOREVERSE_LOUDNESS_THREADS` albums at a time (default: half the cores, at most 4). It
is resumable — an album is written in one go, a restart carries on with the albums still open —
and stops with the server. A file that cannot be decoded is logged, stays `null` and is not
tried again until it changes (size or modification time); the same change makes any file lose
its values and get new ones. So does an album that loses a track or takes one in (a file
deleted, moved between folders, a disc folder now read as part of its album): its tracks are
read and measured again, and until then keep their old values. An album whose files are not
reachable right now is postponed.
`GET /scan/status` reports it as `"loudness": {"running", "total", "processed", "failed"}`
(`total`/`processed` in albums, `failed` in files); the scan card in the web settings shows the
same. `AUTOREVERSE_LOUDNESS=0` switches the measuring off; tags are still read.

**An existing library** needs nothing done: after the update, the server reads the loudness
tags of every track once (at start and with each scan) and then measures the rest in the
background. Decoding takes roughly a tenth of a second per minute of audio and thread on a
desktop CPU — a four-minute track about 0.45 s, so 20,000 tracks take some 40 minutes with four
threads, and a few hours on a small NAS. Until a track is through, its fields are `null` and it
plays unlevelled.

**The web player** levels with the element's `volume`, which ends at 1.0: it turns loud music
down and never boosts. The target of −18 LUFS is low enough that nearly all music is turned
down; a recording quieter than that plays as it is. Safari on iOS ignores `volume`, so nothing
is levelled there.

### Track radio

The UIs call this "Titel-Radio" ("Titel-Radio starten"), to tell it from the internet radio
[stations](#radio-stations). `GET /radio` answers with tracks from the library that go with a seed: what a client queues when
the listener asks for "more like this", or plays on with when its queue runs out. It needs no
language model and asks no outside service.

| Parameter  | Type    | Meaning                                                                        |
| ---------- | ------- | ------------------------------------------------------------------------------ |
| `track_id` | integer | Seed: a track. The answer never contains this track.                           |
| `item_id`  | integer | Seed: an album. The answer contains no track of this album.                    |
| `artist`   | string  | Seed: an artist by name (album artist, artist, or a track's own artist; case does not matter). The answer does contain that artist. |
| `limit`    | integer | How many tracks at most, 1–200, default 50.                                    |
| `exclude`  | integer | A track id to leave out — what the client has queued or just played. Repeat the parameter, at most 500 times. |

Exactly one of `track_id`, `item_id`, `artist`.

```sh
curl -s "http://localhost:8180/radio?track_id=9051&limit=30&exclude=9060&exclude=9077" \
  -H "Authorization: Bearer $TOKEN"
```

```json
{
  "tracks": [
    { "id": 9102, "title": "Isi", "artist": null, "duration": 305.2,
      "item_id": 412, "item_title": "Neu! 75", "item_kind": "album", "has_lyrics": false },
    { "id": 8841, "title": "Europe Endless", "artist": "Kraftwerk", "duration": 575,
      "item_id": 398, "item_title": "Trans Europa Express", "item_kind": "album", "has_lyrics": true }
  ]
}
```

Each entry is the `TrackSummary` that `/search` and playlists use:

| Field        | Type            | Content                                                              |
| ------------ | --------------- | -------------------------------------------------------------------- |
| `id`         | integer         | Track id — for `/tracks/{id}/stream`.                                |
| `title`      | string or null  | The track's title; `null` when its tags have none.                   |
| `artist`     | string or null  | The track's **own** artist from its tags; `null` when it has none (then the album's artist applies, see `/items/{item_id}`). |
| `duration`   | number          | Seconds.                                                             |
| `item_id`    | integer         | The album — for `/items/{id}/cover` and `/play/{id}`.                |
| `item_title` | string          | The album's title.                                                   |
| `item_kind`  | string          | Always `album` here.                                                 |
| `has_lyrics` | boolean         | `GET /tracks/{id}/lyrics` has a text for it, see [Lyrics](#lyrics).  |
| `gain_db`, `album_gain_db`, `peak` | number or null | For levelling the volume, see [Loudness](#loudness). |

What the answer holds, always: music only; nothing from albums whose files are gone or that are
hidden as duplicates; none of the `exclude` tracks; not the seed track; no track twice. It can
be shorter than `limit`, and **an empty list is a valid answer** (`{"tracks": []}`) — a small
library, or everything excluded.

| Status | `detail`                                                                      | When                                     |
| ------ | ----------------------------------------------------------------------------- | ---------------------------------------- |
| `404`  | "Unbekannter Track" / "Unbekanntes Item" / "Interpret nicht gefunden"        | The seed does not exist (an artist: no present album or track by that name). |
| `422`  | "Genau einer der Parameter track_id, item_id oder artist muss angegeben sein" | No seed, more than one, or an empty `artist`. |
| `422`  | "Titel-Radio gibt es nur für Musik"                                           | The seed is a book, a podcast or an episode, or a track of one. |
| `422`  | "track_id muss eine ganze Zahl sein" (likewise `item_id`, `exclude`)          | Not a whole number.                      |
| `422`  | "limit außerhalb des gültigen Bereichs"                                       | `limit` outside 1–200.                   |
| `422`  | "Höchstens 500 exclude-Werte je Anfrage"                                      | More than 500 `exclude`.                 |

How it chooses. Every album present is scored against the seed: a shared genre counts most (a tag
like `Rock; Pop` names two; `Indie Rock` and `Rock` count half), then the seed's artist (also as a
track's artist on a sampler), then the year (full within 3 years, nothing beyond 15). On top comes
the asking user's taste — albums and tracks marked as favorites, and albums heard for five
minutes or more, weigh a little — and a share of chance, so two calls give different lists. The
best albums each give one track per round, closest first: a radio begins near its seed and opens
up. No album gives more than 3 tracks; the seed's artist gets at most an eighth of the list beside
a track or an album seed (a third for an `artist` seed), and so does every other artist, unless
the list would otherwise stay short. No artist plays three times in a row while another one is
left to put in between. An artist seed stands for the genres of that artist's albums and their
middle year. Without genre tags the same works on artist, year, taste and chance. Whatever the
size of the library this takes two queries.

To play a radio track without disturbing its album's progress, open a session for `item_id`,
stream the track and close with `"position": null` (see [Listening time](#listening-time)) — or
play it without a session and report it through `POST /me/listens` with its `track_id`.

### Radio stations

Internet radio: every user keeps a list of public stations and plays them **through this
server**. A device only ever talks to the server — one address, one login — and the server reads
the running title out of the stream. `GET /server/info` says whether the feature is there
(`radio`) and whether stations can be searched (`radio_directory`); with `AUTOREVERSE_RADIO=0`
every route below answers `404` "Radio ist auf diesem Server abgeschaltet".

A station on the wire:

```json
{ "id": 12, "name": "Jazz Eins", "homepage": "https://sender.example/", "country": "DE",
  "tags": ["jazz", "smooth jazz"], "codec": "MP3", "bitrate": 192, "has_logo": true }
```

`homepage`, `country` (ISO 3166-1 alpha-2), `codec` and `bitrate` (kbit/s) are `null` where
unknown. The stream's own address is never sent to a client.

| Method and path                         | Answer                                                                                   |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| `GET /radio/stations`                   | `200 {"stations": [Station…]}` in the user's order.                                      |
| `GET /radio/stations/search?q=&country=&limit=` | `200 {"results": […]}` from the directory: `q` 1–100 characters, `country` two letters (optional), `limit` 1–50 (default 25). Each result is a station without `id`, with `directory_id`, `has_logo` (a logo is worth asking for: the directory names one, or a homepage whose icon can stand in — show it with the route below, which may still answer `404`), `logo_url` (the directory's address, may be `null`; kept for older clients, do not load it) and `added` (the user has it already). Stations the directory reports as broken, and HLS stations, are left out. `404` "Die Sendersuche ist auf diesem Server abgeschaltet" with `AUTOREVERSE_RADIO_DIRECTORY=0`, `502` when the directory cannot be reached. |
| `GET /radio/stations/search/logo?id=&size=` | The logo of a search hit: `id` is its `directory_id`, `size` as for covers. `200` with the image, `404` "Kein Logo" otherwise, see below. |
| `POST /radio/stations`                  | Body `{"directory_id": "…"}` (data and logo from the directory) or `{"stream_url": "…", "name": "…", "homepage": "…"?}` (the logo is then the icon of that homepage, if it has one). `201 Station`; `200` with the existing station when the user has it already (same `directory_id`, same address, or the same stream behind a playlist file). |
| `PATCH /radio/stations/{id}`            | Body `{"name"?: "…", "position"?: n}` — rename, move (1-based, past the end is the end; positions stay gapless). `200 Station`. |
| `DELETE /radio/stations/{id}`           | `204`.                                                                                   |
| `GET /radio/stations/{id}/stream`       | The live stream, see below.                                                              |
| `GET /radio/stations/{id}/now`          | `200 {"title", "artist", "raw", "listening"}`, see below.                                |
| `GET /radio/stations/{id}/logo?size=`   | The stored logo (`size` as for covers; scaled down, a transparent one stays a PNG), `404` "Kein Logo" without one. |

Every `{id}` route answers `404` "Unbekannter Sender" for an unknown id and for another user's
station alike.

**Adding.** Before a station is stored the server fetches the address itself: it must be
`http(s)`, reachable, and answer with audio. Playlist files (`.m3u`, `.pls`) are followed to the
stream they name. Otherwise `422` with a German sentence for the user — among them "Die Adresse
muss mit http:// oder https:// beginnen.", "Der Sender ist unter dieser Adresse nicht
erreichbar.", "Unter dieser Adresse gibt es keinen Audio-Stream. …", "Adressen im lokalen Netz
können nicht als Sender hinzugefügt werden." and, for HLS, "HLS-Sender (.m3u8) werden noch nicht
unterstützt. Bitte die MP3- oder AAC-Adresse des Senders verwenden.". An unknown `directory_id`
is `404`, an unreachable directory `502`. A list holds at most 200 stations.

**HLS is not supported** (for now): an `.m3u8` is not one stream but a playlist of ever new
segment files, which the server would have to rewrite and fetch on a client's word. Such
stations are refused when added and filtered out of the search. What clients get is therefore
always one continuous MP3, AAC or Ogg stream, which iOS, Android, Qt and browsers play natively.

**The stream.** `GET /radio/stations/{id}/stream` is authenticated like `/tracks/{id}/stream`
(bearer token or session cookie) and passes the sender's audio on as it arrives: `200`, the
sender's `Content-Type`, no `Content-Length`, `Accept-Ranges: none` (a `Range` header is
ignored), `Cache-Control: no-store`. The server asks the sender for ICY metadata, takes it out
of the stream — the client receives plain audio — and remembers the title. Old SHOUTcast v1
senders, which answer `ICY 200 OK` instead of `HTTP/1.x 200 OK`, are understood as well, when
adding and when playing; to the client they look like any other. When the client
closes the connection, the connection to the sender ends with it. `HEAD` answers with the
headers without calling the sender. `502` "Sender nicht erreichbar" when the sender does not
answer within ten seconds, answers with an error, or no longer sends audio; `429` "Zu viele
Radio-Streams gleichzeitig" (with `Retry-After`) beyond four streams per user at once. A sender
that goes silent for 20 seconds ends the stream; clients should then reconnect.

**Now playing.** `GET /radio/stations/{id}/now` gives the title last read from the stream:

```json
{ "title": "Autobahn", "artist": "Kraftwerk", "raw": "Kraftwerk - Autobahn", "listening": true }
```

`raw` is the sender's text. `artist` and `title` are set only where it can be taken apart without
guessing — exactly one " - " — and `null` otherwise ("Nachrichten", "A - B - C"): show `raw`
then. `listening` says whether a stream of this station is running through the server right now.
Without one, all three are `null`: the server does not tune in by itself to answer this. Ask
every 10–15 seconds **while playing**; the first title appears a moment after the stream starts.
Senders without ICY metadata (many Ogg streams) never have one.

**Listening time.** Radio is not booked: no session, no progress, nothing under "continue
listening", and nothing in `/me/stats` or `/me/history` — the ledger counts items of the library,
and a live stream is none. Clients must not call `/play`, `/progress` or `/me/listens` for a
station.

**Logos.** Clients load no logo from a third party: a stored station's comes from
`GET /radio/stations/{id}/logo`, a search hit's from `GET /radio/stations/search/logo?id=<directory_id>`
— ask for it where a hit says `has_logo: true`, lazily, with the `size` the row needs, and show
a placeholder on `404`. **`has_logo` on a hit means "worth asking for", not "there is one"**: the
directory names a logo or a homepage, and nothing of either has been fetched when the search
answers — the search never waits for other servers. `404` "Kein Logo" is the answer whenever
no logo can be shown (none to be found, its server unreachable or slower than a few seconds,
larger than 2 MB, no picture). The route takes an id and never an address; the server fetches
what the directory names for that id, with the same fence against the local network as for
streams.

Where the directory names no logo, or one that cannot be had (both are common: the field is
filled in by volunteers), the server reads the station's homepage and takes its icon: an
`apple-touch-icon` first, then `rel="icon"` by declared size, then `/apple-touch-icon.png` and
`/favicon.ico`, and `og:image` last (often a photo rather than a logo). A picture with an edge
under 32 pixels is only taken when nothing better can be had. At most five requests per
homepage (the page counts, at most 512 kB of it are read) within eight seconds, every one of
them — the page, each icon, each redirect — through the fenced client, so a page cannot point
the server into the local network. Stations that name the same homepage share one look at it.
The first request for such a logo can therefore take a few seconds; load logos lazily and
never block a list on them.

The route answers with `Cache-Control: private, max-age=86400` and an `ETag`,
and keeps what it fetched — and what it could not fetch — in memory for a while, so asking
again is cheap. Accepted are JPEG, PNG, WebP, GIF and Windows icon files (`.ico`, served and
stored as the PNG of their largest picture), told by their content; SVG is not (it can carry
scripts and references to other servers, and the server does not render it). With a `size`, a
larger logo comes as a JPEG, or as a PNG where it is transparent.

A station that is added keeps the logo found this way — the directory's, else its homepage's
icon; a station added by address has the icon of the `homepage` given with it. A stored
station that has no logo yet (from the directory, or with a homepage) gets one in the
background when the station list is read — the list itself is not held up, `has_logo` is true
on a later call. On a stored station `has_logo` keeps its strict meaning: the logo is there.

**Privacy.** The directory is [radio-browser.info](https://www.radio-browser.info/), a free
community catalogue. The server sends it the search words (and a country code) when a user
searches, and a station's id when it is added or when its logo is wanted and the server does
not remember the directory's answer; nothing about users or the library. **The server also
fetches the station's own web page** — the homepage the directory names, or the one given when
a station is added by address — and the icons that page names, whenever a logo is wanted and
the directory has none that can be had: when a search hit's logo is asked for, when a station
is added, and in the background for stored stations without a logo (this last part also with
`AUTOREVERSE_RADIO_DIRECTORY=0`). Those requests carry nothing but the `Autoreverse/<version>`
user agent. Senders, their web pages and the servers of logos see the server's address, not
the listener's.

### Remote control

The devices of one account see what plays on each other, steer each other and hand playback
over ("Fernsteuerung" in the UIs). `GET /server/info` says `devices: true` where a server offers
it; with `AUTOREVERSE_DEVICES=0` every route below answers `404` "Die Fernsteuerung ist auf
diesem Server abgeschaltet".

**The server only passes things on.** It keeps the devices in memory, per account, and nothing
in the database: after a restart it knows no device, and the devices reconnect and report again
by themselves. Server → device is one long `GET` of server-sent events; device → server are
ordinary requests. Authentication as everywhere (bearer token or session cookie).

A device makes itself a `device_id` once and keeps it: 8–64 characters out of `[A-Za-z0-9-]`
(a UUID). Ids are per account — the same id in two accounts is two devices.

| Request                                   | Answer                                                                 |
| ----------------------------------------- | ---------------------------------------------------------------------- |
| `GET /me/devices/events?device_id=&name=&kind=` | `200 text/event-stream`, see below. `name` 1–60 characters, `kind` one of `desktop`, `phone`, `tablet`, `web`, `car`; `422` otherwise. |
| `GET /me/devices`                         | `200 {"devices": [Device…]}` — the same list, for a client without a stream. |
| `PUT /me/devices/{device_id}/state`       | Body `State`. `204`; `422` for a state out of shape or bounds; `409` "Das Gerät ist nicht verbunden" for a device that has not opened its stream. |
| `POST /me/devices/{device_id}/commands`   | Body `{"command", "value"?, "from": device_id}`. `202`; `409` when the target is unknown, offline or has no stream at the moment; `422` for a bad command or value; `429` beyond 20 commands a second to one device. |
| `POST /me/devices/{device_id}/transfer`   | Body `{"from": device_id \| null, "play"?: bool}` (`play` defaults to `true`). `202`; `409` without anything to take, or when the target is not connected. |
| `DELETE /me/devices/{device_id}`          | `204`. The device signs off (on logout): gone at once, its stream ended. |

**The stream.** The response carries `Cache-Control: no-cache` and `X-Accel-Buffering: no`. While
it is open the device is online. When it breaks, the device stays listed for a grace period of
20 seconds; a reconnect within it is invisible to the others. A second connection with the same
`device_id` replaces the first, which the server ends. Every 15 seconds a comment line `: ping`
is sent. Events, each an `event:` line and one `data:` line of JSON:

| Event       | Data                                                                                |
| ----------- | ----------------------------------------------------------------------------------- |
| `devices`   | `{"devices": [Device…]}` — right after connecting and whenever something changes: a device comes or goes, a name, the active device, a state in a way that matters (anything but the position moving on with the clock; a seek counts). |
| `command`   | `{"command": string, "value": number \| string \| bool \| null, "from": device_id}` — for this device. |
| `take_over` | `{"state": State, "from": device_id, "play": bool}` — this device is to play that state. |

```json
{ "id": "3f2b8c1e-6c0a-4a57-9a3e-0d1f6b7c8d9e", "name": "Oskars Telefon", "kind": "phone",
  "online": true, "active": true,
  "state": {
    "playing": true,
    "media": { "kind": "track", "item_id": 7, "track_id": 71, "station_id": null,
               "title": "Crying", "artist": "Björk", "item_title": "Debut" },
    "position": 42.5, "duration": 289.0, "volume": 0.8, "shuffle": false, "repeat": "off",
    "queue": [{ "item_id": 7, "track_id": 71 }, { "item_id": 7, "track_id": 72 }],
    "context": "Debut", "at": "2026-11-13T12:00:00.000Z" } }
```

Only online devices are listed (`online` is always `true`), sorted by name. `state` is `null`
until the device reported one. `active` marks at most one device: the one that last reported
`playing: true`, or that last took playback over; it stays active while paused, and nobody is
active once it goes offline.

**State.** A device reports what it plays at once when something changes, else every 10 seconds
while it plays. `position` and `duration` are seconds within the running track for music, within
the whole book or episode otherwise. `volume` is 0–1, or `null` where it cannot be set from afar.
`repeat` is `off`, `all` or `one`. `media.kind` is `album`, `book`, `episode`, `track` or
`station`. `queue` (at most 300) is the list from the running entry on, running entry first: a
track names `item_id` and `track_id`, a whole book or episode `item_id` only. `context` is a line
to show (an album's name, "Titel-Radio: …"). `at` is when the device measured, any ISO-8601 time
with an offset; the server sends it on as UTC **with milliseconds** (`…T12:00:00.000Z` — unlike
the other timestamps of this API). Watchers work out the position themselves: `position` plus
(now − `at`) while `playing`. Fields a device leaves out come back with their defaults (`media`,
`volume`, `context`: `null`; `duration`: 0; `shuffle`: false; `repeat`: `off`; `queue`: `[]`);
`playing`, `position` and `at` are required. Report once after every (re)connect of the stream:
a restarted server has forgotten the state, and `PUT …/state` is `409` until the stream is open.

**Commands.** `play`, `pause`, `toggle`, `next`, `previous`, `stop` take no value; `seek` takes
seconds (≥ 0, in the same terms as `position`), `volume` 0–1, `shuffle` a bool, `repeat` `off`,
`all` or `one`. A device ignores what it cannot do. `from` is the sender's own `device_id`; the
sender needs no stream. Commands only ever reach devices of the caller's own account — anybody
else's device is simply unknown (`409`).

**Handing over.** `POST …/transfer` sends the target `take_over` with the state the source last
reported — `from`, or with `null` the active device — its `position` moved on to now (by the
server's clock since the report came in; `at` is then the server's time), and sends the source
the command `stop`. The target builds its playback from it — a book or episode: `POST
/play/{item_id}` and seek to `position`; music: the `queue` as a list of tracks, starting at
`position` in the first; a station: `station_id` — and then reports its own state. Only that
report makes it the active device (also when it took over paused, `play: false`).

**Limits.** At most 20 devices per account; one more pushes out the one that was silent longest.
A stream that is not read has a bounded buffer; when it runs over the server ends the stream and
the device reconnects. A logout ends the streams of its token at once; a token that stops being
valid any other way (password change, deleted account) ends its streams within 30 seconds.

**The web player** is a device of kind `web` ("Firefox auf Linux"), with its id in
`localStorage`. All tabs of a browser are that one device; exactly one of them holds the stream
(the one that plays, agreed over a `BroadcastChannel`). The "Geräte" button of the mini bar and
the fullscreen player lists the devices; a click moves playback there. While another device
plays and the browser does not, the mini bar is its remote: "Läuft auf …", play/pause,
previous/next, position, volume and "Hier weiterhören". The web player has no shuffle or repeat
and ignores those two commands.

### Playlists

A playlist belongs to the account that made it. Its owner can share it with **the other accounts
of this server** — nothing here is ever public — and open it for their entries too. Everything
below is additive: a client that knows none of it keeps working, and `GET /playlists` without a
parameter still lists the user's own playlists only.

**The playlist object** — in `GET /playlists`, `GET /playlists/{id}` (there with `entries`), and
in every answer that returns a playlist (`POST /playlists`, `PATCH`, `PUT …/cover`, the import):

```json
{
  "id": 7, "name": "Zugfahrt", "created_at": "2026-11-13T18:04:11Z", "entry_count": 42,
  "description": "Für lange Strecken", "folder": "Reisen", "has_cover": true,
  "owner": "oskar", "mine": true, "shared": true, "collaborative": false,
  "can_edit": true, "duration": 9874.5
}
```

| Field           | Type           |                                                                                     |
| --------------- | -------------- | ----------------------------------------------------------------------------------- |
| `description`   | string \| null |                                                                                     |
| `folder`        | string \| null | A folder's name. Playlists with the same name form a folder; there is no folder object and no nesting. |
| `has_cover`     | bool           | An uploaded cover exists. Without one, compose a mosaic from the entries as before. |
| `owner`         | string         | The owner's account name.                                                           |
| `mine`          | bool           | The asking account is the owner.                                                    |
| `shared`        | bool           | Visible to every account of this server.                                            |
| `collaborative` | bool           | Other accounts may add, remove and move entries. Reported as chosen, but only in effect while `shared`. |
| `can_edit`      | bool           | The asking account may change the entries: the owner, or anybody on a shared, collaborative playlist. |
| `duration`      | number         | Seconds, all entries added up (a whole-item entry counts all its tracks; an episode not downloaded yet what its feed claims). Always present. |

**`GET /playlists?scope=mine|shared|all`** — `mine` (the default) is the user's own, `shared` the
shared playlists of *other* accounts, `all` both: own first, then the others', each by name.
Anything else is `422`.

**`PATCH /playlists/{id}`** takes, beside `name`, any of `description` (≤ 2000 characters; empty
or `null` clears it), `folder` (≤ 100 characters, trimmed; empty or `null` = no folder), `shared`
and `collaborative` (booleans). A field that is absent stays as it is; a body with none of the
five is `422` as before. `POST /playlists` accepts the same four beside its `name`.

**Who may do what**

| | owner | other account, private | …, shared | …, shared and collaborative |
| --- | --- | --- | --- | --- |
| read, play, cover, export | yes | `404` | yes | yes |
| add, remove, move entries | yes | `404` | `403` | yes |
| name, description, folder, sharing, cover, delete | yes | `404` | `403` | `403` |

A private playlist of somebody else is `404 "Unbekannte Playlist"` on every route, exactly like
one that does not exist — also for admins.

**Cover.** `PUT /playlists/{id}/cover` takes the picture either as a multipart form with the file
in the field `image` (as `POST /artists/{name}/image` does) or as the request body itself with
its `Content-Type`. JPEG, PNG, WebP or GIF, at most 5 MB (`413` beyond, `422` for another type
or an empty file). It answers `200` with the playlist. `DELETE` removes it (`204`).
`GET /playlists/{id}/cover?size=<pixels>` serves it like every cover (thumbnail sizes, `ETag`) —
and is `404 "Kein Cover"` when the playlist has none of its own: the server does not make one up.

**Export.** `GET /playlists/{id}/export?format=m3u8` (the default) answers
`Content-Type: audio/x-mpegurl; charset=utf-8` and `Content-Disposition: attachment;
filename="<name>.m3u8"; filename*=UTF-8''<name>.m3u8` — the name without what a file system
refuses, the first form in ASCII. The body is Extended M3U:

```
#EXTM3U
#PLAYLIST:Zugfahrt
#EXTINF:252,Björk - Human Behaviour
Björk/Debut/01 Human Behaviour.flac
```

One `#EXTINF:<seconds, rounded>,<artist> - <title>` per track (the title alone where no artist
is known) and below it the file's path **relative to the root of its library** — absolute paths
of the server never leave it; a file under no root is named by its file name only. An entry that
is a whole item (an album, a book) is resolved into its tracks. `?format=json` gives the same
list for tools: `{"name", "description", "tracks": [{"title", "artist", "album", "duration",
"path"}]}`.

**Import.** `POST /playlists/import` with `{"name": "…", "content": "<M3U or M3U8 text, at most
2 MB>", "folder": "…"?}` answers `201`:

```json
{ "playlist": { … }, "matched": 40, "unmatched": ["The Knife - Heartbeats", "Mix/99 Fehlt.mp3"] }
```

Each line is matched in this order:

1. **By path**: the longest run of folders and file name it shares with a track of the
   collection, counted from the end — so a path relative to a library root matches as well as
   one under a foreign root (`/Volumes/Musik/…`, `C:\Musik\…`), with backslashes, URL encoding
   or `file://`. Case and accents do not matter. A match on the bare file name alone counts only
   if it is unambiguous and the line's `#EXTINF` does not contradict it.
2. **By `#EXTINF`**: artist and title, compared without case, accents and additions in brackets
   ("(Remastered 2011)"); with several hits the one whose length is within ±3 s of the
   `#EXTINF`'s wins.

The file's order is kept and doubles stay. What was not found comes back in `unmatched` as
readable lines (the `#EXTINF` text, else the path). If **nothing** was found, no playlist is
created: `422` with `{"detail": "…", "unmatched": […]}`. Text beyond 2 MB is `413`.

### Sharing a link

`GET /server/info` names `web_url`: the public address of the web UI (`ORIGIN`), or `null` when
the operator did not set one. Clients build links to its pages from it. **Whoever opens such a
link needs an account on this server**; there are no public links without a login. The paths
are stable:

| What          | Path                                           |
| ------------- | ---------------------------------------------- |
| Album         | `/library/albums/{id}`                         |
| A track       | `/library/albums/{album id}?track={track id}` — the album's page with that row marked and scrolled to |
| Audiobook     | `/library/books/{id}`                          |
| Podcast       | `/library/podcasts/{id}`                       |
| Artist        | `/library/albums?artist={name, URL-encoded}` — the artist's albums |
| Playlist      | `/library/playlists/{id}`                      |

The web UI has "Link kopieren" on each of these pages; it copies the address under the origin
the browser itself uses.

### Discover feed

`GET /ai/feed` is everything the "Entdecken" page shows for the asking user. Despite the path it
works without a language model: only `suggestions`, `mixes`, `artists` and `wanted` come from one
(and stay empty without it). Every list is always present, empty when there is nothing to show.

| Field              | Type                | From         | Content                                                                 |
| ------------------ | ------------------- | ------------ | ----------------------------------------------------------------------- |
| `enabled`          | boolean             | —            | Whether a model is set up.                                              |
| `refreshing`       | boolean             | —            | A background run for the model-made lists is under way; ask again in a few seconds. |
| `has_history`      | boolean             | —            | The user has listening time or favorites.                               |
| `checking_catalog` | boolean             | —            | Still comparing with MusicBrainz; more `wanted` may follow.             |
| `generated_at`     | string or null      | —            | When the model-made lists were generated.                               |
| `truncated`        | boolean             | —            | The library was too large to show the model all of it.                  |
| `suggestions`      | album suggestion[]  | model        | Albums of the library to hear next, each with a `reason`.               |
| `mixes`            | mix[]               | model        | `{name, description, cover_ids, tracks}`; `tracks` as `/ai/playlist` returns them. |
| `artists`          | artist[]            | model        | `{name, reason, albums}`: artists the library lacks.                    |
| `wanted`           | wanted album[]      | model        | `{artist, title, year, reason}`: albums the library lacks.              |
| `rediscover`       | album suggestion[]  | database     | Albums heard before, but not for 90 days, the most heard first.         |
| `fresh`            | album suggestion[]  | database     | Albums added in the last 60 days, unheard, by an artist or in a genre the user hears. |
| `new_episodes`     | episode[]           | database     | The newest unheard episodes of the subscribed podcasts, at most 12.     |
| `unstarted_books`  | book[]              | database     | Audiobooks the user has not begun, latest additions first, at most 12.  |
| `continue_series`  | book[]              | database     | For each series the user finished a book of, the next volume not begun, at most 12. |
| `rediscover_books` | book[]              | database     | Audiobooks begun, not finished and not touched for 30 days, at most 12. |

An album suggestion is `{id, title, artist, year, has_cover, reason}`; `reason` is empty in the
lists that come from the database.

The lists marked "database" are computed on every request, not stored with the model's answer:
an episode that just arrived, or a book just begun, is reflected in the very next response.
`new_episodes`, `unstarted_books`, `continue_series` and `rediscover_books` were added later; a
client must treat them as absent-means-empty when talking to an older server.

An **episode** is the same object as in `GET /podcasts/new`, and `new_episodes` is the head of
that list (unfinished episodes published since the podcast was subscribed, newest first):

| Field           | Type           | Meaning                                                                       |
| --------------- | -------------- | ----------------------------------------------------------------------------- |
| `id`            | integer        | The episode's item id — for `/play/{id}`, `/items/{id}`, `/episodes/{id}/download`. |
| `title`         | string         |                                                                               |
| `published_at`  | string or null | ISO-8601, UTC.                                                                |
| `podcast_id`    | integer        | The podcast's item id (the episode's parent).                                 |
| `podcast_title` | string         |                                                                               |
| `has_cover`     | boolean        | `GET /items/{id}/cover` has an image — the episode's own or its podcast's.    |
| `downloaded`    | boolean        | Whether the server holds the file. If not, `POST /episodes/{id}/download` first; `/play/{id}` answers `409` until then. |
| `duration`      | number         | Seconds: measured once downloaded, else what the feed claims, else `0`.       |
| `position`      | number         | The user's saved position in seconds, `0` if not begun.                       |

A **book** is the item summary of `GET /items` with three more fields, so a client can decode it
with the model it already has:

| Field           | Type           | Meaning                                                                 |
| --------------- | -------------- | ----------------------------------------------------------------------- |
| `id`            | integer        | Item id — for `/play/{id}`, `/items/{id}`, `/items/{id}/cover`.         |
| `kind`          | string         | Always `"book"` here.                                                   |
| `title`         | string         |                                                                         |
| `has_cover`     | boolean        |                                                                         |
| `author`        | string or null |                                                                         |
| `artist`        | string or null | `null` for books.                                                       |
| `series`        | string or null | Name of the series.                                                     |
| `year`          | integer or null |                                                                        |
| `missing_since` | string or null | Always `null` here: missing items are left out.                         |
| `published_at`  | string or null | `null` for books.                                                       |
| `narrator`      | string or null | Added to the summary.                                                   |
| `series_index`  | number or null | The volume number within `series` (may be fractional, e.g. `2.5`). Part of every item summary. |
| `duration`      | number         | Added: total length in seconds, `0` if unknown.                         |
| `position`      | number         | Added: the user's saved position in seconds; `0` except in `rediscover_books`. |

```json
{
  "enabled": false, "refreshing": false, "has_history": true, "checking_catalog": false,
  "generated_at": null, "truncated": false,
  "suggestions": [], "mixes": [], "artists": [], "wanted": [], "rediscover": [], "fresh": [],
  "new_episodes": [
    { "id": 2291, "title": "Der Untergang der Batavia", "published_at": "2026-11-11T05:00:00Z",
      "podcast_id": 310, "podcast_title": "Zeitzeichen", "has_cover": true, "downloaded": false,
      "duration": 892, "position": 0 }
  ],
  "unstarted_books": [
    { "id": 412, "kind": "book", "title": "Der Schwarm", "has_cover": true,
      "author": "Frank Schätzing", "artist": null, "series": null, "year": 2004,
      "missing_since": null, "published_at": null,
      "narrator": "Stefan Kaminski", "series_index": null, "duration": 133200, "position": 0 }
  ],
  "continue_series": [
    { "id": 97, "kind": "book", "title": "Tehanu", "has_cover": true,
      "author": "Ursula K. Le Guin", "artist": null, "series": "Erdsee", "year": 1990,
      "missing_since": null, "published_at": null,
      "narrator": null, "series_index": 4, "duration": 28800, "position": 0 }
  ],
  "rediscover_books": []
}
```

How the book lists are chosen: a book counts as begun once the user has a position greater than
0 in it or has finished it. A series is told apart by its name **and** author. Its volumes are
ordered by `series_index`, then by a number in the title, then by title; "next" is the first
volume not begun after the furthest one finished — after volume 3, volume 4 is offered even if
volume 1 was skipped, and nothing once the last volume is finished or begun. The series finished
in most recently comes first. A book can appear in both `unstarted_books` and `continue_series`.

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
build machine (`e2e/run-in-vm.sh`), not on the developer's desktop; adapt the script if you
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
  library/      queries, progress, bookmarks, favorites, playlists, track radio, metadata editing,
                lyrics, most played and the year in review
  podcasts/     feed parsing, directory search, episode downloads, retention, refresh job
  stations/     internet radio: directory, checking an address, the stream proxy, ICY titles
  devices/      remote control: the online devices per account, their event streams
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
