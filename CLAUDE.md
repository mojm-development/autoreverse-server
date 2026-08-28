# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Autoreverse Server: a self-hosted media server for audiobooks, music and podcasts. SvelteKit
(Svelte 5, runes forced on for non-`node_modules` files in `vite.config.ts`) on `adapter-node`,
Postgres via Drizzle, package manager and script runner is **bun**. One process serves both a
JSON API for native clients and a server-rendered web UI.

The code is a port of an earlier Python/FastAPI+pydantic implementation. Comments across
`src/lib/server/` refer to that "ground truth" (`api/auth.py`, `_run_scan`, `BackgroundTasks`) and
to review IDs (`C-2`, `I-6`, `M-12`, `Ruling 8`). Those references explain _why_ a shape exists;
the wire format is deliberately kept byte-compatible with the Python original, including its
snake_case field names, except where a comment marks an intentional divergence.

## Commands

```sh
bun install
bun run db:start          # docker compose up (Postgres on host port 5434)
bun run db:push           # sync schema.ts to the dev DB, no migration file
bun run dev               # http://localhost:5173

bun run lint              # prettier --check + eslint
bun run format            # prettier --write
bun run check             # svelte-check / tsc
bun run build && bun run preview

bun run db:generate       # generate a migration from schema.ts changes
bun run db:migrate        # apply migrations
bun run db:studio
```

Tests:

```sh
bun run test:unit --run                       # both vitest projects
bun run test:unit --run tests/unit/tags       # one file / path filter
bun run test:unit --run --project server      # 'server' (node) or 'client' (browser) only
bun run test:e2e                              # playwright; builds + previews on :4173
bunx playwright test tests/e2e/login.e2e.ts   # one e2e spec
bun run test                                  # unit then e2e
```

`bun run build` and every `db:*` script need `DATABASE_URL` exported — bun only auto-loads `.env`
for processes it runs itself, and `vite`/`drizzle-kit`/`node build` run under Node. Copy
`.env.example` to `.env` and export, or use a process manager. `ffprobe`/`ffmpeg` must be on PATH
(m4b chapter extraction; `tests/unit/tags.test.ts` also shells out to `ffmpeg` to build fixtures).

## Architecture

### Routes are two things in one tree

`src/routes/` holds the JSON API at the top level (`+server.ts`: `/items`, `/scan`, `/playlists`,
…) and the German web UI under `/library`, `/settings`, `/login` (`+page.svelte` +
`+page.server.ts`). Page `load` functions and form actions call `$lib/server/…` **directly**, not
over HTTP; only a few interactive components `fetch()` the API from the browser.

### The `_xxxHandler(db, event)` convention

Every API route splits into a testable handler plus a thin binding — all 37 `+server.ts` files
follow this, keep it:

```ts
export async function _itemsGetHandler(db: DrizzleDb, event: Pick<RequestEvent, 'locals' | 'url'>) {
	try {
		requireApiUser(event.locals);
		…
		return json(…);
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _itemsGetHandler(defaultDb, event);
```

Integration tests import the underscore-prefixed handler and invoke it through
`tests/integration/_callRoute.ts` with a throwaway database — no HTTP server, no SvelteKit runtime.
The `_` prefix is what makes SvelteKit accept a non-handler export from a `+server.ts`.

Errors: throw `ApiError(status, germanDetail, retryAfter?)` from `$lib/server/api/errors`, convert
at the route boundary with `apiError()`. Every response body is `{ detail: string }` — including
uncaught ones, which `handleError` in `src/hooks.server.ts` backfills. Page routes instead throw
`redirect()` (see `requireWebUser` / `requireWebAdmin`). Use `readJson()` and `intParam()` from
`$lib/server/api/validate` rather than bare `request.json()` / manual parsing, so malformed input
becomes a 422 with `detail` instead of a 500.

Wire serialization lives in `$lib/server/api/serialize.ts`: camelCase Drizzle rows in, snake_case
API objects out, timestamps through `toIso()` (which strips `.000` fractional seconds — strict
ISO-8601 clients reject them). Do not hand-roll row→JSON mapping in a route.

### Auth

`src/hooks.server.ts` resolves a bearer header first, else the `autoreverse_session` cookie, into
`locals.userId` / `locals.token` for every request — one resolver behind both API and web. Guards
live in `$lib/server/auth/session.ts` (`requireApiUser`, `requireApiAdmin`, `requireWebUser`,
`requireWebAdmin`). Passwords are argon2id (`@node-rs/argon2`) with a concurrency semaphore and a
login throttle in `auth/semaphore.ts` + `auth/throttle.ts`. `init` in hooks also runs
`ensureFirstAdmin` and, when `AUTOREVERSE_AUTO_MIGRATE` is set (the Docker image sets it), applies
migrations — deliberately opt-in, since `db:push`-synced dev databases have no migration history.

### Scanning

`scanner/run.ts` walks the books root and the music root as **two fully separate passes with
distinct `root` arguments**. `root` scopes `markMissing`; merging the passes or swapping roots
would flag the other library's items as missing. An unreadable root is skipped entirely rather
than scanned as empty, for the same reason. Per-folder failures are collected and their items left
untouched.

Scan status is a module-level singleton in `$lib/server/admin/scanState.ts` — one per server
process, mirroring FastAPI's `app.state`. `POST /scan` mutates it and fires `runScan()` without
awaiting (the FastAPI `BackgroundTasks` equivalent), so `/scan/status` can poll live progress.
**Consequence for tests: no two e2e specs may trigger a real scan concurrently** — real scans race
through this singleton and through the `library_config` row.

### Library paths

The books and music roots are a single row in the `library_config` table
(`$lib/server/settings/libraryPaths.ts`), configured at runtime under Settings → Bibliotheken, not
via environment variables. Env only decides `AUTOREVERSE_DATA` (covers, downloaded episodes) and
`DATABASE_URL`. Behind a reverse proxy `ORIGIN` must be set or form POSTs are rejected.

## Testing layers

- `tests/unit/` — pure functions (tags, chapters, ranges, tokens, throttle), plain vitest.
- `tests/integration/` — route handlers and query modules against a real Postgres.
  `withTestDb()` in `tests/fixtures.ts` starts a fresh `PostgreSqlContainer` and runs
  `drizzle-kit push` **per call**, so Docker is required and timeouts are raised to 60s in
  `vite.config.ts`. Wrap each test's body in `withTestDb`.
- `src/**/*.svelte.test.ts` — component tests, vitest `client` project, real Chromium via
  `vitest-browser-svelte`.
- `tests/e2e/` — Playwright against `bun run build && bun run preview`. Uses the **shared dev
  Postgres on :5434**, not a container, so specs must be idempotent and must not assume a clean
  database (`auth.setup.ts` creates its users only if absent). Authenticated specs inherit
  `playwright/.auth/user.json`; a spec that needs to be logged out must opt out with
  `test.use({ storageState: { cookies: [], origins: [] } })`.

`expect: { requireAssertions: true }` is on — a test with no assertion fails.

## Conventions

- **All user-facing strings are German**, including API `detail` messages, and tests assert on the
  exact wording. Code, comments and identifiers are English.
- Prettier: tabs, single quotes, no trailing commas, 100 columns. Run `bun run format`.
- Styling is plain CSS with design tokens in `src/lib/styles/tokens.css` (dark by default,
  `:root[data-theme='light']` override, fixed per-content-type accents `--music` / `--book` /
  `--podcast`) plus shared utility classes in `src/app.css` (`.eyebrow`, `.mono`, `.pill`,
  `.badge`). Prefer existing tokens and classes over new local values.
- Schema changes go in `src/lib/server/db/schema.ts` and must ship with a generated migration
  (`bun run db:generate`); check constraints and partial/unique indexes are expressed there, not
  in application code.
- Commit messages are conventional-commit style with a scope where useful
  (`fix(scanner): …`, `feat(scan): …`).
