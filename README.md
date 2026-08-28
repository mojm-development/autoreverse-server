# Autoreverse Server

A self-hosted media server for **audiobooks, music and podcasts** — with a JSON API for native
clients and a web UI you can use on its own.

Autoreverse scans your existing folders of audio files, reads their tags, extracts chapters and
cover art, and serves everything over HTTP with per-user playback progress, bookmarks, favorites
and playlists. Podcasts are subscribed by feed, browsable through a directory search, and
episodes can be downloaded to the server for offline access.

It runs as a single Node process backed by Postgres. No transcoding daemon, no plugin system, no
media-server sprawl — one container and a database.

> **Deutsche Fassung dieser Datei: [README.de.md](README.de.md).**
> Note that the **web UI is currently German-only**; the JSON API is language-neutral.

---

## Status

Early but real. The scanner, player, podcast subscriptions, user management and the full API
surface below all work; the project is at version `0.0.1` and the database schema may still change
between releases without a migration path. Run it against data you have backups of.

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
- Per-feed refresh, episodes downloaded to server-side storage on demand

**Users**

- Argon2id password hashing (`@node-rs/argon2`)
- Bearer tokens for API clients, session cookies for the web UI, one resolver behind both
- Login throttling
- Admin/non-admin split: user creation and library configuration are admin-only
- Per-user playback preferences

---

## Quick start (Docker Compose)

A ready-built image is published to GHCR as
[`ghcr.io/mojm-development/autoreverse-server`](https://ghcr.io/mojm-development/autoreverse-server)
(`latest` follows `main`; version tags follow releases). Save the following as `compose.yaml`,
adjust the two media mounts and the admin credentials, and you are done — the image applies
database migrations on boot, no manual schema step needed:

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
      # Downloaded podcast episodes, reachable from the host. Writable, unlike the
      # two mounts above. Drop this line to keep them inside the volume.
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

Either way, the server is then on <http://localhost:8180>. Log in with the
`AUTOREVERSE_ADMIN_USER` / `AUTOREVERSE_ADMIN_PASSWORD` from your `compose.yaml` — **change those
before the first start**, they create your admin account.

Then, still in the web UI:

1. **Settings → Bibliotheken**: enter the _in-container_ paths, `/library/books` and
   `/library/music`. Library roots live in the database, not in the environment (see below).
2. **Settings → Scan**: start the first scan.

---

## Configuration

| Variable                     | Required        | Default                                                           | Purpose                                                                                               |
| ---------------------------- | --------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`               | yes             | `postgresql://autoreverse:autoreverse@localhost:5434/autoreverse` | Postgres connection string.                                                                           |
| `AUTOREVERSE_DATA`           | no              | `./data`                                                          | Base directory for server-managed files. `covers/`, `podcasts/` and `artists/` are created inside it. |
| `AUTOREVERSE_ADMIN_USER`     | first boot only | —                                                                 | Username of the first admin account.                                                                  |
| `AUTOREVERSE_ADMIN_PASSWORD` | first boot only | —                                                                 | Password of that account.                                                                             |
| `AUTOREVERSE_AUTO_MIGRATE`   | no              | unset (`1` in the Docker image)                                   | Apply pending database migrations on boot. Leave unset for dev databases synced via `db:push`.        |
| `ORIGIN`                     | behind a proxy  | derived from the `Host` header, assuming `http`                   | The public URL of the server. See [Running behind a reverse proxy](#running-behind-a-reverse-proxy).  |

See [`.env.example`](.env.example) for a copy-paste starting point.

### Running behind a reverse proxy

If anything terminates TLS in front of Autoreverse — Caddy, nginx, Traefik — set `ORIGIN` to the
public URL, exactly as the browser sees it and without a trailing slash:

```yaml
environment:
  ORIGIN: https://autoreverse.example.com
```

Skip this and the web UI login fails with **`Cross-site POST form submissions are forbidden`**.
The reason: SvelteKit compares the browser's `Origin` header against the server's own origin on
every form POST. Behind a TLS-terminating proxy the browser reports `https://your.domain` while
the Node process, which only ever sees plain HTTP on port 3000, believes it is `http://…:3000`.
The two disagree, so the POST is rejected. Only form submissions are affected — the JSON API
authenticates fine, which is why the failure shows up at the login screen specifically.

If you would rather not hard-code the hostname, the alternative is to let the proxy tell the
server (Caddy sets both headers by default):

```yaml
environment:
  PROTOCOL_HEADER: x-forwarded-proto
  HOST_HEADER: x-forwarded-host
```

Prefer `ORIGIN` where you can. The header variant trusts whatever sends those headers, so it is
only safe if nothing but your proxy can reach the container.

### Getting at downloaded podcast episodes

Episodes downloaded through the UI are written to `$AUTOREVERSE_DATA/podcasts`, which lives inside
the container's data volume by default — fine for playback, awkward if you also want the files on
your NAS. Mount that one directory through to the host to change that:

```yaml
volumes:
  - /srv/media/podcasts:/data/podcasts # writable, unlike the library mounts
```

Two things to know before you do. The mount must be **writable** — the library mounts are `:ro`,
this one cannot be. And the files are named after their episode id, not their title: `/data/podcasts/42.mp3`,
not `Some Show - Episode 42.mp3`. They are the server's own storage that you are looking in on,
not an export.

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
nobody can log in — user creation is admin-only, so there is no second bootstrap path. Set both
before the very first boot against a fresh database. Leaving them set afterwards is harmless; they
are only read while the table is still empty.

### External dependency

`ffprobe` (part of **ffmpeg**) must be on `PATH` for `.m4b` chapter extraction. The provided
Docker image installs it; on a bare-metal install, install ffmpeg yourself. Without it, everything
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
to the same user. Errors come back as JSON with a `detail` field, and `retryAfter` where a
throttle applies.

| Method                 | Path                                              | Purpose                                                                |
| ---------------------- | ------------------------------------------------- | ---------------------------------------------------------------------- |
| `POST`                 | `/auth/login`                                     | Exchange credentials for a token                                       |
| `POST`                 | `/auth/logout`                                    | Revoke the current token                                               |
| `GET`                  | `/me`                                             | Current user                                                           |
| `POST`                 | `/me/password`                                    | Change own password                                                    |
| `GET` `PUT`            | `/me/playback`                                    | Playback preferences                                                   |
| `GET`                  | `/items`                                          | List items — `?kind=book\|album\|podcast\|episode`, `?q=`, `?missing=` |
| `GET`                  | `/items/{id}`                                     | One item                                                               |
| `GET`                  | `/items/{id}/children`                            | Tracks / episodes of an item                                           |
| `GET`                  | `/items/{id}/cover`                               | Cover image                                                            |
| `DELETE`               | `/items/missing`                                  | Purge items whose files are gone (admin)                               |
| `GET`                  | `/artists`                                        | Distinct artists                                                       |
| `GET` `POST`           | `/artists/{name}/image`                           | Custom artist image; upload is admin, multipart `image`                |
| `PUT` `DELETE`         | `/artists/{name}/cover`                           | Pick an album as the artist image, or reset to random (admin)          |
| `GET`                  | `/search`                                         | Cross-library search                                                   |
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
| `POST`                 | `/podcasts`                                       | Subscribe to a feed                                                    |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Refresh / unsubscribe                                                  |
| `POST`                 | `/episodes/{id}/download`                         | Download an episode to the server                                      |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Library scan control (admin)                                           |
| `GET` `POST`           | `/users`, `/users/{id}`                           | User administration (admin)                                            |
| `GET`                  | `/server/info`                                    | Library counts; user count for admins                                  |

---

## Development

```sh
bun install
bun run db:start      # docker compose up -d postgres
bun run db:push       # sync the Drizzle schema to that database
bun run dev           # http://localhost:5173
```

### Build

```sh
bun run build
bun run preview       # serve the production build locally
```

`bun run build` and every `db:*` script need `DATABASE_URL` in the environment. `adapter-node`
produces a plain Node server, so nothing loads `.env` for you outside `vite dev` — export the
variables or use a process manager that does. `bun run` does **not** close that gap: bun only
auto-loads `.env` for processes it runs itself, and these scripts' binaries (`vite`,
`drizzle-kit`, `node build`) run under Node.

### Database

```sh
bun run db:push       # dev schema sync, no migration file
bun run db:generate   # create a migration from schema.ts changes
bun run db:migrate    # apply migrations
bun run db:studio     # Drizzle Studio
```

### Tests

```sh
bun run test:unit --run   # vitest; integration suites spin up Postgres via testcontainers (needs Docker)
bun run test:e2e          # playwright (needs docker compose up -d postgres)
bun run test              # both
```

### Checks

```sh
bun run lint          # prettier --check + eslint
bun run format        # prettier --write
bun run check         # svelte-check / TypeScript
```

### Layout

```
src/lib/server/
  auth/         sessions, tokens, password hashing, login throttling, bootstrap
  db/           Drizzle schema and connection
  library/      queries, progress, bookmarks, favorites, playlists
  podcasts/     feed parsing, directory search, episode downloads
  scanner/      book and music scans, tags, chapters, covers
  settings/     runtime-configured library paths
  streaming/    HTTP range responses
src/lib/components/   Svelte UI components
src/routes/           JSON API endpoints (+server.ts) and web UI pages
drizzle/              generated migrations
tests/                unit, integration and e2e suites
```

---

## Contributing

Issues and pull requests are welcome.

- Run `bun run lint`, `bun run check` and `bun run test:unit --run` before opening a PR.
- Keep schema changes accompanied by a generated migration (`bun run db:generate`).
- The UI is German; please match the existing wording and tone in user-facing strings.

---

## License

Licensed under the **GNU Affero General Public License v3.0**. See [LICENSE](LICENSE).

The AGPL's network clause matters here: if you run a modified Autoreverse and let other people use
it over a network, you have to offer them the modified source as well.
