# Plan: Autoreverse Server in Rust

Abgestimmt am 2026-10-14. Branch `feature/rust-server`.

## Entscheidungen

- **Alles in Rust, kein Node/Bun im Betrieb.** axum + tokio + sqlx (Postgres). HTML serverseitig
  per askama-Templates; Interaktivität mit htmx und kleinen Vanilla-JS-Modulen (Player, Scrubber,
  Visualizer, Infinite Scroll). `tokens.css` und `app.css` werden unverändert übernommen, die
  scoped Styles der Svelte-Komponenten wandern in statisches CSS — das Aussehen bleibt gleich.
- **Frisches Schema** mit sqlx-Migrationen, keine Rücksicht auf vorhandene Daten. Nach dem Update
  wird neu gescannt und es werden Nutzer neu angelegt.
- **Direkte Vollumstellung.** Der TS-Code liegt bis zur letzten Scheibe als Referenz unter
  `legacy/` und wird dann gelöscht. Rust liegt im Repo-Root.
- **Bibliotheken:** lofty (Tags), argon2, feed-rs, reqwest, tower-http, tracing; ffprobe/ffmpeg
  bleiben externe Prozesse.
- **Die JSON-API bleibt wire-kompatibel zur iOS-App:** gleiche Pfade und Methoden, snake_case,
  `{ "detail": "<deutsch>" }` als Fehlerkörper, ISO-Zeitstempel ohne `.000`.
- **Tests:** Unit-Tests inline; Integrationstests über `tower::ServiceExt::oneshot` gegen Postgres
  (`#[sqlx::test]`, eine DB pro Test) — die Fälle aus `legacy/tests/integration` werden portiert.
  E2E bleibt Playwright (Specs aus `legacy/tests/e2e`, an das Markup angepasst) und läuft in einer
  Tart-VM auf Jarvis gegen den lokal laufenden Server.

## Scheiben

Jede Scheibe ist durchgehend (Migration, Queries, API, HTML, Tests) und endet mit einem Commit.

| # | Verhalten | API | Web |
|---|---|---|---|
| 1 | Anmelden; Server startet, Schema wird angelegt, erster Admin aus Env | `/auth/login`, `/auth/logout`, `/me`, `/server/info` | `/login`, Layout + Sidebar, Logout, Theme-Toggle |
| 2 | Hörbücher scannen und ansehen | `/scan*`, `/items`, `/items/{id}`, `/items/{id}/cover`, `/items/series` | Settings → Bibliotheken + Scan, `/library/books[/id]`, `/library/series` |
| 3 | Musik scannen und ansehen (zweiter Pass mit eigenem `root`) | `/items/{id}/children`, `/tracks/{id}`, `/artists*` | Alben, Album-Detail, Interpreten, Interpret-Detail |
| 4 | Abspielen und Fortschritt | `/tracks/{id}/stream` (Range), `/play/{itemId}`, `/progress*`, `/sessions/{id}/close`, `/me/stats`, `/me/playback` | Start „Weiterhören“, Fullscreen-Player, MiniPlayer, Settings → Wiedergabe |
| 5 | Favoriten, Lesezeichen, Suche | `/favorites*`, `/bookmarks*`, `/search` | die drei Seiten |
| 6 | Playlists | `/playlists*` | Liste + Detail |
| 7 | Podcasts: abonnieren, stündlicher Refresh, Download, Retention, Verzeichnissuche | `/podcasts*`, `/episodes/{id}/download` | Podcasts, Vorschau, Detail, Settings → Podcasts |
| 8 | Metadaten bearbeiten (einzeln, Masse, Undo) | `PATCH /items/{id}`, `/items/bulk*`, `/items/missing` | MetadataEditor, BulkEditor, Auswahlleiste |
| 9 | Nutzer und Sicherheit | `/users*`, `/me/password`, Login-Throttle | Settings → Nutzer, Sicherheit, Über |
| 10 | Ausliefern | Dockerfile (Rust-Build, debian-slim + ffmpeg), `legacy/` weg, CLAUDE.md/README neu, kompletter E2E-Lauf, Draft-PR | – |

## Arbeitsweise in der Nacht

- Nach jeder Scheibe gezielt `cargo test` der betroffenen Module, `cargo clippy`, `cargo fmt`,
  passende E2E-Specs auf Jarvis; danach ein Commit (Deutsch, Body = warum).
- Hängt eine Scheibe, wird das im Commit und im PR vermerkt und die nächste begonnen.
- Der Draft-PR enthält je Scheibe: Neu, Testschritte, automatisch abgedeckt, noch nicht.

## Stand

2026-10-15, nach Scheibe 10. Alle zehn Scheiben sind umgesetzt, dazu die Lücken aus dem Audit
gegen legacy (`GET /library/more`, Download-Sperre je Folge, Randfälle der API, Erkennung einer
alten Datenbank) und der Playlist-Player. `legacy/` ist gelöscht; der alte Stand ist
`0b627bd`, `e2e/design/compare.sh` holt ihn bei Bedarf per `git archive`.

- **Automatisch grün:** `cargo test` (185 Unit-, 406 Integrationstests), clippy `-D warnings`,
  fmt, 62 JS-Tests (`node --test 'tests/js/*.test.mjs'`).
- **Docker:** Image baut für arm64 (nativ) und amd64 (cross); Smoke-Test mit compose und frischer
  Datenbank lief (`/server/info`, API- und Formular-Login, `/login` mit CSS und htmx,
  `set-password` im Container, Healthcheck).
- **Offen:**
  - Kompletter E2E-Lauf (Playwright auf Jarvis) und der Pixel-Vergleich (`e2e/design/compare.sh`).
    SSH zu Jarvis war in der Nacht gesperrt (1Password). Der Design-Vergleich lief nur statisch,
    ohne Browser (`e2e/design/styles.mjs`).
  - Draft-PR: nicht angelegt, der Branch ist nicht gepusht.
  - Das CI-Image (`.github/workflows/docker.yml`) ist erst beim nächsten Push auf `main` geprüft.
