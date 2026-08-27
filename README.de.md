# Autoreverse Server

Ein selbstgehosteter Medienserver für **Hörbücher, Musik und Podcasts** — mit JSON-API für native
Clients und einer Web-Oberfläche, die auch für sich allein genügt.

Autoreverse durchsucht deine vorhandenen Ordner mit Audiodateien, liest deren Tags, extrahiert
Kapitel und Cover und liefert alles über HTTP aus — inklusive Hörfortschritt, Lesezeichen,
Favoriten und Wiedergabelisten pro Benutzer. Podcasts abonnierst du per Feed, findest sie über
eine Verzeichnissuche, und einzelne Episoden lassen sich auf den Server herunterladen.

Der Server läuft als einzelner Node-Prozess mit Postgres dahinter. Kein Transcoding-Daemon, kein
Plugin-System, kein Medienserver-Wildwuchs — ein Container und eine Datenbank.

> **English version of this file: [README.md](README.md).**

---

## Stand

Früh, aber echt nutzbar. Scanner, Player, Podcast-Abos, Benutzerverwaltung und die unten
dokumentierte API funktionieren; das Projekt steht bei Version `0.0.1`, und das Datenbankschema
kann sich zwischen Releases noch ohne Migrationspfad ändern. Setz es auf Daten an, von denen du
Backups hast.

---

## Funktionen

**Bibliothek**

- Rekursiver Scan eines Hörbuch- und eines Musikverzeichnisses, strikt getrennt voneinander
- Audioformate: `.mp3`, `.m4a`, `.m4b`, `.flac`, `.ogg`, `.opus`, `.wav`, `.aac`
- Metadaten aus den Tags: Autor, Sprecher, Reihe und Reihenposition bei Büchern; Interpret,
  Album-Interpret und Jahr bei Musik
- Kapitel aus `.m4b`-Dateien (per `ffprobe`, das den `chpl`-Atom liest, an dem Tag-Bibliotheken
  scheitern)
- Cover aus den Tags oder aus dem Ordner, serverseitig zwischengespeichert
- Verschwundene Dateien werden als fehlend markiert statt stillschweigend gelöscht — Aufräumen
  bleibt eine bewusste Entscheidung
- Scans lassen sich abbrechen und melden ihren Fortschritt

**Wiedergabe**

- HTTP-Range-Streaming — Springen funktioniert, Clients laden nur, was sie abspielen
- Hörposition pro Benutzer, geräteübergreifend fortgesetzt, plus „Weiterhören"-Liste
- Lesezeichen mit Zeitstempel, Favoriten für ganze Titel wie für einzelne Tracks
- Wiedergabelisten mit sortierten Einträgen
- Wiedergabesitzungen: du siehst, was gerade anderswo läuft, und kannst es beenden

**Podcasts**

- Verzeichnissuche (öffentliche iTunes-Search-API von Apple) — nach Namen suchen statt Feed-URLs
  zusammensuchen
- Feed-Vorschau vor dem Abonnieren
- Aktualisierung je Feed, Episoden auf Wunsch in den Serverspeicher heruntergeladen

**Benutzer**

- Passwort-Hashing mit Argon2id (`@node-rs/argon2`)
- Bearer-Token für API-Clients, Session-Cookies für die Weboberfläche, ein gemeinsamer Resolver
- Drosselung bei Fehlanmeldungen
- Trennung Verwalter/Benutzer: Benutzeranlage und Bibliothekskonfiguration nur für Verwalter
- Wiedergabeeinstellungen pro Benutzer

---

## Schnellstart (Docker Compose)

Das Repository bringt eine `compose.yaml` mit, die Postgres und den Server zusammen startet.

```sh
git clone https://github.com/mojm-development/autoreverse-server.git
cd autoreverse-server

# Auf deine eigenen Medien zeigen lassen; beide werden nur lesend eingebunden.
export AUTOREVERSE_BOOKS_HOST=/srv/media/hoerbuecher
export AUTOREVERSE_MUSIC_HOST=/srv/media/musik

docker compose up -d
```

Der Server läuft dann auf <http://localhost:8180>. Melde dich mit `AUTOREVERSE_ADMIN_USER` /
`AUTOREVERSE_ADMIN_PASSWORD` aus der `compose.yaml` an — **ändere die vor dem ersten Start**, sie
legen dein Verwalterkonto an.

Danach, weiterhin in der Weboberfläche:

1. **Einstellungen → Bibliotheken**: die Pfade _im Container_ eintragen, also `/library/books` und
   `/library/music`. Die Bibliothekspfade stehen in der Datenbank, nicht in der Umgebung (siehe
   unten).
2. **Einstellungen → Scan**: den ersten Scan starten.

---

## Konfiguration

| Variable                     | Pflicht               | Standard                                                          | Zweck                                                                                             |
| ---------------------------- | --------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`               | ja                    | `postgresql://autoreverse:autoreverse@localhost:5434/autoreverse` | Postgres-Verbindungsstring.                                                                       |
| `AUTOREVERSE_DATA`           | nein                  | `./data`                                                          | Basisverzeichnis für servereigene Dateien. `covers/` und `podcasts/` entstehen darin automatisch. |
| `AUTOREVERSE_ADMIN_USER`     | nur beim ersten Start | —                                                                 | Benutzername des ersten Verwalterkontos.                                                          |
| `AUTOREVERSE_ADMIN_PASSWORD` | nur beim ersten Start | —                                                                 | Passwort dieses Kontos.                                                                           |

Eine Vorlage zum Kopieren liegt in [`.env.example`](.env.example).

### Bibliothekspfade sind keine Umgebungsvariablen

Die Wurzelverzeichnisse für Hörbücher und Musik liegen **in der Datenbank** und werden zur Laufzeit
unter _Einstellungen → Bibliotheken_ gesetzt (nur Verwalter). Beim Deployment musst du lediglich
dafür sorgen, dass die Pfade, die du dort eintragen willst, tatsächlich eingebunden und für den
Serverprozess lesbar sind. Das ist Absicht: So kann ein Verwalter eine Bibliothek umhängen, ohne
neu zu deployen.

### Erstes Verwalterkonto

Bei jedem Start prüft der Server, ob die Tabelle `users` leer ist. Ist sie das **und** sind
`AUTOREVERSE_ADMIN_USER` und `AUTOREVERSE_ADMIN_PASSWORD` beide gesetzt, wird dieser Benutzer als
erster Verwalter angelegt.

Ist die Tabelle leer und die Variablen fehlen, startet der Server trotzdem, schreibt aber eine
Warnung ins Log — und niemand kann sich anmelden, denn Benutzer anlegen dürfen nur Verwalter, einen
zweiten Weg gibt es nicht. Setze also beide Variablen vor dem allerersten Start gegen eine frische
Datenbank. Sie danach stehen zu lassen, schadet nicht: gelesen werden sie nur, solange die Tabelle
leer ist.

### Externe Abhängigkeit

`ffprobe` (Teil von **ffmpeg**) muss im `PATH` liegen, damit Kapitel aus `.m4b`-Dateien gelesen
werden. Das mitgelieferte Docker-Image installiert es; bei einer Installation ohne Container musst
du ffmpeg selbst bereitstellen. Ohne ffprobe läuft alles andere weiter — `.m4b`-Dateien kommen dann
eben ohne Kapitel an.

---

## HTTP-API

Einmal anmelden, danach das Token als Bearer-Header mitschicken:

```sh
TOKEN=$(curl -s -X POST http://localhost:8180/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"name":"admin","password":"change-me"}' | jq -r .token)

curl -s http://localhost:8180/items?kind=book -H "Authorization: Bearer $TOKEN"
```

Anfragen aus der Weboberfläche authentifizieren sich stattdessen über das Cookie
`autoreverse_session`; beide Wege landen beim selben Benutzer. Fehler kommen als JSON mit einem
Feld `detail` zurück, bei Drosselung zusätzlich mit `retryAfter`.

| Methode                | Pfad                                              | Zweck                                                                       |
| ---------------------- | ------------------------------------------------- | --------------------------------------------------------------------------- |
| `POST`                 | `/auth/login`                                     | Zugangsdaten gegen ein Token tauschen                                       |
| `POST`                 | `/auth/logout`                                    | Aktuelles Token widerrufen                                                  |
| `GET`                  | `/me`                                             | Angemeldeter Benutzer                                                       |
| `POST`                 | `/me/password`                                    | Eigenes Passwort ändern                                                     |
| `GET` `PUT`            | `/me/playback`                                    | Wiedergabeeinstellungen                                                     |
| `GET`                  | `/items`                                          | Titel auflisten — `?kind=book\|album\|podcast\|episode`, `?q=`, `?missing=` |
| `GET`                  | `/items/{id}`                                     | Einzelner Titel                                                             |
| `GET`                  | `/items/{id}/children`                            | Tracks bzw. Episoden eines Titels                                           |
| `GET`                  | `/items/{id}/cover`                               | Coverbild                                                                   |
| `DELETE`               | `/items/missing`                                  | Fehlende Titel entfernen (Verwalter)                                        |
| `GET`                  | `/artists`                                        | Interpreten                                                                 |
| `GET`                  | `/search`                                         | Suche über die ganze Bibliothek                                             |
| `GET` `HEAD`           | `/tracks/{id}/stream`                             | Audiostream mit Range-Unterstützung                                         |
| `POST`                 | `/play/{itemId}`                                  | Wiedergabesitzung öffnen                                                    |
| `POST`                 | `/sessions/{sessionId}/close`                     | Wiedergabesitzung schließen                                                 |
| `GET` `PUT/POST`       | `/progress/{itemId}`, `/progress/continue`        | Hörfortschritt und „Weiterhören"                                            |
| `GET` `POST` `DELETE`  | `/bookmarks`, `/bookmarks/{id}`                   | Lesezeichen                                                                 |
| `GET`                  | `/favorites`                                      | Favoriten                                                                   |
| `POST` `DELETE`        | `/favorites/items/{id}`, `/favorites/tracks/{id}` | Favoriten setzen/entfernen                                                  |
| `GET` `POST`           | `/playlists`                                      | Wiedergabelisten auflisten / anlegen                                        |
| `GET` `PATCH` `DELETE` | `/playlists/{id}`                                 | Lesen / umbenennen / löschen                                                |
| `POST` `PUT` `DELETE`  | `/playlists/{id}/entries[/{entryId}]`             | Einträge einer Wiedergabeliste                                              |
| `GET`                  | `/podcasts/search`                                | Verzeichnissuche nach Namen                                                 |
| `POST`                 | `/podcasts`                                       | Feed abonnieren                                                             |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Aktualisieren / Abo beenden                                                 |
| `POST`                 | `/episodes/{id}/download`                         | Episode auf den Server laden                                                |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Scan steuern (Verwalter)                                                    |
| `GET` `POST`           | `/users`, `/users/{id}`                           | Benutzerverwaltung (Verwalter)                                              |
| `GET`                  | `/server/info`                                    | Bestandszahlen, für Verwalter auch Benutzerzahl                             |

---

## Entwicklung

```sh
bun install
bun run db:start      # docker compose up -d postgres
bun run db:push       # Drizzle-Schema in die Datenbank spiegeln
bun run dev           # http://localhost:5173
```

### Bauen

```sh
bun run build
bun run preview       # Produktions-Build lokal ausliefern
```

`bun run build` und alle `db:*`-Skripte brauchen `DATABASE_URL` in der Umgebung. `adapter-node`
erzeugt einen gewöhnlichen Node-Server, deshalb lädt außerhalb von `vite dev` niemand die `.env`
für dich — exportiere die Variablen oder nimm einen Prozessmanager, der das tut. `bun run` schließt
diese Lücke **nicht**: bun lädt `.env` nur für Prozesse, die es selbst startet, und die Binaries
dieser Skripte (`vite`, `drizzle-kit`, `node build`) laufen unter Node.

### Datenbank

```sh
bun run db:push       # Schema-Abgleich für die Entwicklung, ohne Migrationsdatei
bun run db:generate   # Migration aus Änderungen an schema.ts erzeugen
bun run db:migrate    # Migrationen anwenden
bun run db:studio     # Drizzle Studio
```

### Tests

```sh
bun run test:unit --run   # vitest; Integrationstests starten Postgres per testcontainers (braucht Docker)
bun run test:e2e          # playwright (braucht docker compose up -d postgres)
bun run test              # beides
```

### Prüfungen

```sh
bun run lint          # prettier --check + eslint
bun run format        # prettier --write
bun run check         # svelte-check / TypeScript
```

### Aufbau

```
src/lib/server/
  auth/         Sitzungen, Token, Passwort-Hashing, Anmeldedrosselung, Bootstrap
  db/           Drizzle-Schema und Verbindung
  library/      Abfragen, Fortschritt, Lesezeichen, Favoriten, Wiedergabelisten
  podcasts/     Feed-Verarbeitung, Verzeichnissuche, Episoden-Downloads
  scanner/      Buch- und Musikscan, Tags, Kapitel, Cover
  settings/     zur Laufzeit gesetzte Bibliothekspfade
  streaming/    HTTP-Range-Antworten
src/lib/components/   Svelte-Komponenten der Oberfläche
src/routes/           JSON-API-Endpunkte (+server.ts) und Weboberfläche
drizzle/              erzeugte Migrationen
tests/                Unit-, Integrations- und E2E-Tests
```

---

## Mitmachen

Issues und Pull Requests sind willkommen.

- Vor einem PR bitte `bun run lint`, `bun run check` und `bun run test:unit --run` laufen lassen.
- Schemaänderungen gehören zusammen mit einer erzeugten Migration ins Repository
  (`bun run db:generate`).
- Die Oberfläche ist deutsch; bitte halte dich bei sichtbaren Texten an die vorhandene Wortwahl.

---

## Lizenz

Lizenziert unter der **GNU Affero General Public License v3.0**, siehe [LICENSE](LICENSE).

Die Netzwerk-Klausel der AGPL ist hier der springende Punkt: Wer eine veränderte Fassung von
Autoreverse betreibt und andere über ein Netzwerk darauf zugreifen lässt, muss ihnen auch den
veränderten Quellcode anbieten.
