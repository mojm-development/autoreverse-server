# Autoreverse Server

Autoreverse — a self-hosted server for audiobooks, music and podcast subscriptions, with a
native-client-compatible JSON API and a German-language web UI. Postgres-backed (via Drizzle
ORM), deployed as a single Node process. Originally built as a SvelteKit rewrite of the Python
app it replaces.

## Environment variables

| Variable                     | Required            | Default                                                           | Purpose                                                                                                                                                     |
| ---------------------------- | ------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`               | yes                 | `postgresql://autoreverse:autoreverse@localhost:5434/autoreverse` | Postgres connection string.                                                                                                                                 |
| `AUTOREVERSE_DATA`           | no                  | `./data`                                                          | Base directory for server-managed files (covers, downloaded podcast episodes). Subdirectories `covers/` and `podcasts/` are created under it automatically. |
| `AUTOREVERSE_ADMIN_USER`     | only for first boot | —                                                                 | Username for the first admin account, created automatically the first time the server starts against an **empty** `users` table.                            |
| `AUTOREVERSE_ADMIN_PASSWORD` | only for first boot | —                                                                 | Password for that first admin account.                                                                                                                      |

**Library paths (`booksDir`/`musicDir`) are _not_ env vars.** Unlike the original Python app,
Autoreverse stores the audiobook/music library root paths in the database, configured at
runtime from the web UI (Settings → Bibliotheken, admin-only). There is nothing to set for these
at deploy time beyond making sure the paths you'll type into that UI are actually mounted/visible
to the server process.

## First-admin bootstrap

On every boot, the server checks whether the `users` table is empty. If it is **and**
`AUTOREVERSE_ADMIN_USER`/`AUTOREVERSE_ADMIN_PASSWORD` are both set, it creates that user as the
first admin. If the table is empty and those vars are unset, the server still starts, but logs a
warning and nobody can log in until an admin is created some other way (there is no other
bootstrap path — user creation is admin-only). Set both vars before the very first boot against a
fresh database; they're safe to leave set afterwards (they're only read while the table is
still empty).

## Developing

```sh
pnpm install
pnpm db:start           # docker compose up -d postgres (local Postgres)
pnpm db:push            # sync the Drizzle schema to that database
pnpm dev                # start the dev server
```

## Building

```sh
pnpm build
pnpm preview             # serve the production build locally
```

`pnpm build` and any `pnpm db:*` command need `DATABASE_URL` set (see `.env.example`);
`adapter-node`'s output is a plain Node server, so nothing loads `.env` for you outside `vite
dev` — export the variables or use a process manager that does.

## Database

```sh
pnpm db:push             # dev schema sync, no migration file
pnpm db:generate         # create a migration from schema.ts changes
pnpm db:migrate          # apply migrations
pnpm db:studio           # Drizzle Studio
```

## Tests

```sh
pnpm test:unit -- --run  # vitest, single run (needs Docker for the testcontainer-backed suites)
pnpm test:e2e             # playwright install + playwright test (needs docker compose up -d postgres)
pnpm test                 # both of the above
```
