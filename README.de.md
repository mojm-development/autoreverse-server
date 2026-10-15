# Autoreverse Server

Ein selbstgehosteter Medienserver für **Hörbücher, Musik und Podcasts** — mit JSON-API für native
Clients und einer Web-Oberfläche, die auch für sich allein genügt.

Autoreverse durchsucht deine vorhandenen Ordner mit Audiodateien, liest deren Tags, extrahiert
Kapitel und Cover und liefert alles über HTTP aus — inklusive Hörfortschritt, Lesezeichen,
Favoriten und Wiedergabelisten pro Benutzer. Podcasts abonnierst du per Feed, findest sie über
eine Verzeichnissuche, und einzelne Episoden lassen sich auf den Server herunterladen.

Der Server ist ein einzelnes Rust-Binary mit Postgres dahinter, `ffprobe` ist sein einziger Helfer
von außen. Kein Transcoding-Daemon, kein Plugin-System, kein Medienserver-Wildwuchs — ein
Container und eine Datenbank.

> **English version of this file: [README.md](README.md).**

---

## Stand

Früh, aber echt nutzbar. Scanner, Player, Podcast-Abos, Benutzerverwaltung und die unten
dokumentierte API funktionieren. Mit Version `0.1.0` wurde der Server von SvelteKit/TypeScript
nach Rust umgeschrieben; die API blieb kompatibel, die Datenbank nicht (siehe
[Umstieg von der TypeScript-Version](#umstieg-von-der-typescript-version)). Setz es auf Daten an,
von denen du Backups hast.

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
- Einträge, deren Dateien verschwunden sind, werden beim nächsten Scan aus der Datenbank
  entfernt — samt Fortschritt, Lesezeichen und Favoriten. Ein unlesbarer Bibliotheks-Ordner wird
  komplett übersprungen und ein fehlgeschlagener Unterordner ausgenommen, ein nicht erreichbares
  Laufwerk leert also nichts
- Scans lassen sich abbrechen und melden ihren Fortschritt
- Metadaten in der Weboberfläche bearbeiten, einzeln und in Masse, mit Rückgängig

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
- Die neuesten N Folgen eines Abos vorhalten — ein globaler Standard, pro Feed überschreibbar.
  Downloads jenseits dieser Zahl werden beim nächsten Aktualisieren wieder entfernt
- Aktualisierung je Feed, Episoden auf Wunsch in den Serverspeicher heruntergeladen

**Benutzer**

- Passwort-Hashing mit Argon2id
- Bearer-Token für API-Clients, Session-Cookies für die Weboberfläche, ein gemeinsamer Resolver
- Drosselung bei Fehlanmeldungen (`429` mit `Retry-After`)
- Trennung Verwalter/Benutzer: Benutzeranlage und Bibliothekskonfiguration nur für Verwalter
- Wiedergabeeinstellungen pro Benutzer

---

## Schnellstart (Docker Compose)

Ein fertig gebautes Image liegt auf GHCR:
[`ghcr.io/mojm-development/autoreverse-server`](https://ghcr.io/mojm-development/autoreverse-server)
(`latest` folgt `main`, Versions-Tags folgen Releases). Speichere Folgendes als `compose.yaml`,
passe die beiden Medien-Einbindungen und die Verwalter-Zugangsdaten an — mehr ist nicht nötig, der
Server spielt seine Datenbankmigrationen bei jedem Start selbst ein. Images gibt es für
`linux/amd64` und `linux/arm64`:

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
      # Erst-Verwalter, nur einmalig gegen eine leere `users`-Tabelle benutzt — ändere das.
      AUTOREVERSE_ADMIN_USER: admin
      AUTOREVERSE_ADMIN_PASSWORD: change-me
      # Pflicht, sobald ein Reverse Proxy davorsteht — die öffentliche URL, so wie
      # der Browser sie sieht. Ohne sie scheitert die Anmeldung mit „Cross-site POST
      # form submissions are forbidden". Weglassen, wenn du den Server direkt erreichst.
      ORIGIN: https://autoreverse.example.com
    volumes:
      # Auf deine eigenen Medien zeigen lassen; beide werden nur lesend eingebunden.
      # Nach dem ersten Login die Container-Pfade (/library/books, /library/music)
      # unter Einstellungen → Bibliotheken eintragen, damit sie benutzt werden.
      - /srv/media/hoerbuecher:/library/books:ro
      - /srv/media/musik:/library/music:ro
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

Alternativ baust du selbst aus dem Quellcode — die `compose.yaml` im Repository baut das Image
lokal, statt es zu ziehen:

```sh
git clone https://github.com/mojm-development/autoreverse-server.git
cd autoreverse-server

# Auf deine eigenen Medien zeigen lassen; beide werden nur lesend eingebunden.
export AUTOREVERSE_BOOKS_HOST=/srv/media/hoerbuecher
export AUTOREVERSE_MUSIC_HOST=/srv/media/musik

# Optional: wohin heruntergeladene Podcast-Episoden auf dem Host sollen.
export AUTOREVERSE_PODCASTS_HOST=/srv/media/podcasts

docker compose up -d
```

So oder so läuft der Server dann auf <http://localhost:8180>. Das Image hat einen eingebauten
Healthcheck (`autoreverse-server healthcheck`), `docker compose ps` zeigt also, wann er bereit ist. Melde dich mit
`AUTOREVERSE_ADMIN_USER` / `AUTOREVERSE_ADMIN_PASSWORD` aus deiner `compose.yaml` an — **ändere
die vor dem ersten Start**, sie legen dein Verwalterkonto an.

Danach, weiterhin in der Weboberfläche:

1. **Einstellungen → Bibliotheken**: die Pfade _im Container_ eintragen, also `/library/books` und
   `/library/music`. Die Bibliothekspfade stehen in der Datenbank, nicht in der Umgebung (siehe
   unten).
2. **Einstellungen → Scan**: den ersten Scan starten.

---

## Konfiguration

| Variable                            | Pflicht               | Standard                                                          | Zweck                                                                                                         |
| ----------------------------------- | --------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`                      | ja                    | `postgresql://autoreverse:autoreverse@localhost:5434/autoreverse` | Postgres-Verbindungsstring.                                                                                   |
| `AUTOREVERSE_DATA`                  | nein                  | `./data` (`/data` im Image)                                       | Basisverzeichnis für servereigene Dateien. `covers/`, `podcasts/` und `artists/` entstehen darin automatisch. |
| `AUTOREVERSE_ADMIN_USER`            | nur beim ersten Start | —                                                                 | Benutzername des ersten Verwalterkontos.                                                                      |
| `AUTOREVERSE_ADMIN_PASSWORD`        | nur beim ersten Start | —                                                                 | Passwort dieses Kontos.                                                                                       |
| `AUTOREVERSE_PODCAST_REFRESH_HOURS` | nein                  | `1`                                                               | Wie oft der Server alle Feeds aktualisiert und die Downloads nachhält. `0` schaltet den Hintergrundlauf ab.   |
| `ORIGIN`                            | hinter einem Proxy    | aus dem `Host`-Header abgeleitet, Protokoll als `http` angenommen | Öffentliche URL des Servers. Siehe [Betrieb hinter einem Reverse Proxy](#betrieb-hinter-einem-reverse-proxy). |
| `HOST` / `PORT`                     | nein                  | `0.0.0.0` / `3000`                                                | Wo der Server lauscht.                                                                                        |
| `AUTOREVERSE_STATIC`                | nein                  | `static/` neben dem Binary, sonst `./static` (`/app/static` im Image) | CSS, JS und Icons. Findet der Server sie nicht, startet er nicht.                                         |
| `RUST_LOG`                          | nein                  | `autoreverse_server=info,tower_http=warn`                         | Log-Filter.                                                                                                   |

Den Schalter `AUTOREVERSE_AUTO_MIGRATE` gibt es nicht mehr: Die Migrationen sind ins Binary
kompiliert und werden bei jedem Start eingespielt.

Eine Vorlage zum Kopieren liegt in [`.env.example`](.env.example).

### Betrieb hinter einem Reverse Proxy

Wenn vor Autoreverse irgendetwas TLS terminiert — Caddy, nginx, Traefik —, setze `ORIGIN` auf die
öffentliche URL, exakt so wie der Browser sie sieht und ohne Schrägstrich am Ende:

```yaml
environment:
  ORIGIN: https://autoreverse.example.com
```

Lässt du das weg, scheitert die Anmeldung in der Weboberfläche mit
**`Cross-site POST form submissions are forbidden`**. Der Grund: Der Server vergleicht bei jedem
Formular-POST den `Origin`-Header des Browsers mit seiner eigenen Origin. Hinter einem
TLS-terminierenden Proxy meldet der Browser `https://deine.domain`, während der Server, der nur
schlichtes HTTP auf Port 3000 zu sehen bekommt, sich für `http://<Host>` hält. Beide weichen
voneinander ab, also wird der POST abgelehnt. Betroffen sind ausschließlich Formulare — die
JSON-API authentifiziert sich einwandfrei, weshalb der Fehler ausgerechnet am Anmeldebildschirm
auftritt. Ein `ORIGIN` mit `https://` setzt außerdem das `Secure`-Flag am Sitzungs-Cookie.

`X-Forwarded-*`-Header wertet der Server nicht aus; `ORIGIN` ist der einzige Weg, ihm seine
öffentliche URL mitzuteilen.

### Heruntergeladene Podcast-Episoden von außen erreichen

Über die Oberfläche heruntergeladene Episoden landen in `$AUTOREVERSE_DATA/podcasts` und damit
standardmäßig im Daten-Volume des Containers — zum Abspielen genügt das, unpraktisch wird es, wenn
du die Dateien auch auf dem NAS haben willst. Dafür reichst du genau dieses Verzeichnis nach außen
durch:

```yaml
volumes:
  - /srv/media/podcasts:/data/podcasts # schreibbar, anders als die Bibliotheks-Mounts
```

Zwei Dinge solltest du vorher wissen. Der Mount muss für den Nutzer im Container, UID 1000,
**schreibbar** sein — die Bibliotheks-Mounts sind `:ro`, dieser darf es nicht sein. Gehört der
Ordner auf dem Host jemand anderem, setze `user: '<uid>:<gid>'` am Dienst. Und du schaust dem
Server bei seiner eigenen Ablage zu, das ist kein Export: ein Ordner je Podcast, die Dateien nach
dem Folgentitel benannt (`/data/podcasts/Irgendeine Sendung/Folge 42.mp3`), Zeichen, die auf
gängigen Dateisystemen nicht erlaubt sind, ersetzt.

Zwischengespeicherte Cover (`$AUTOREVERSE_DATA/covers`) bleiben im Volume; sie sind abgeleitete
Daten und entstehen beim nächsten Scan neu, wenn sie verloren gehen.

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
Warnung ins Log — und niemand kann sich anmelden. Setze also beide Variablen vor dem allerersten
Start gegen eine frische Datenbank. Sie danach stehen zu lassen, schadet nicht: gelesen werden sie
nur, solange die Tabelle leer ist.

Ein vergessenes Passwort oder eine Installation ganz ohne Verwalter behebst du auf der
Kommandozeile. Das Passwort wird abgefragt (oder als erste Zeile von stdin gelesen), nie als
Argument übergeben:

```sh
docker compose exec app /app/autoreverse-server set-password admin --admin
```

Das setzt das Passwort eines vorhandenen Benutzers (und meldet alle seine Sitzungen ab) oder legt
ihn an. `--admin` macht ihn zum Verwalter.

### Umstieg von der TypeScript-Version

Version `0.1.0` ersetzt den SvelteKit-Server durch einen in Rust. HTTP-API, Weboberfläche und die
Ablage unter `AUTOREVERSE_DATA` bleiben gleich, das Datenbankschema wurde aber neu angelegt, und
es gibt **keine automatische Übernahme**. Gegen eine Datenbank der alten Version startet der
Server nicht und sagt das auch. Für den Umstieg:

1. Eine neue, leere Datenbank anlegen (mit der `compose.yaml` oben: `docker compose down -v` wirft
   die alte samt Inhalt weg) und `DATABASE_URL` darauf zeigen lassen.
2. Server starten; der erste Verwalter entsteht wie bisher aus
   `AUTOREVERSE_ADMIN_USER`/`_PASSWORD`.
3. Unter _Einstellungen → Bibliotheken_ die Pfade wieder eintragen, scannen, die übrigen Benutzer
   neu anlegen und Podcasts neu abonnieren. Fortschritt, Lesezeichen, Favoriten und
   Wiedergabelisten werden nicht übernommen.

### Externe Abhängigkeit

`ffprobe` (Teil von **ffmpeg**) muss im `PATH` liegen, damit Kapitel aus `.m4b`-Dateien gelesen
werden. Das mitgelieferte Docker-Image installiert es; bei einer Installation ohne Container musst
du ffmpeg selbst bereitstellen und das Verzeichnis `static/` neben das Binary legen (oder
`AUTOREVERSE_STATIC` setzen). Ohne ffprobe läuft alles andere weiter — `.m4b`-Dateien kommen dann
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
Feld `detail` (deutsch) zurück, bei Drosselung zusätzlich mit dem Header `Retry-After`.
Zeitstempel sind ISO-8601 in UTC mit ganzen Sekunden (`2026-10-14T18:22:57Z`).

`/progress/continue` liefert zu jedem Eintrag zusätzlich `duration` (die Summe der Trackdauern)
und `has_cover` — damit lässt sich ein Fortschrittsbalken zeichnen, ohne das ganze Item zu laden.

`/items` nimmt `?sort=title|added|series|played` (Vorgabe `title`); `played` ordnet nach dem
Fortschritt des fragenden Nutzers. Ein unbekannter Wert ergibt 422 statt stiller Rückfall.

| Methode                | Pfad                                              | Zweck                                                                       |
| ---------------------- | ------------------------------------------------- | --------------------------------------------------------------------------- |
| `POST`                 | `/auth/login`                                     | Zugangsdaten gegen ein Token tauschen                                       |
| `POST`                 | `/auth/logout`                                    | Aktuelles Token widerrufen                                                  |
| `GET`                  | `/me`                                             | Angemeldeter Benutzer                                                       |
| `GET`                  | `/me/stats`                                       | Hörzeit je Tag — `?days=` (Vorgabe 30)                                      |
| `POST`                 | `/me/password`                                    | Eigenes Passwort ändern                                                     |
| `GET` `PUT`            | `/me/playback`                                    | Wiedergabeeinstellungen                                                     |
| `GET`                  | `/items`                                          | Titel auflisten — `?kind=book\|album\|podcast\|episode`, `?q=`, `?missing=` |
| `GET` `PATCH`          | `/items/{id}`                                     | Einzelner Titel; Metadaten bearbeiten (Verwalter)                           |
| `POST`                 | `/items/bulk`, `/items/bulk/undo`                 | Metadaten in Masse bearbeiten und zurücknehmen (Verwalter)                  |
| `POST`                 | `/items/series`                                   | Reihen und Bandnummern erkennen oder zuweisen, vorab als Probelauf (Verwalter) |
| `GET`                  | `/items/{id}/children`                            | Tracks bzw. Episoden eines Titels                                           |
| `GET`                  | `/items/{id}/cover`                               | Coverbild                                                                   |
| `DELETE`               | `/items/missing`                                  | Fehlende Titel entfernen (Verwalter)                                        |
| `GET`                  | `/artists`                                        | Interpreten                                                                 |
| `GET`                  | `/library/more`                                   | Nächste Seite des Bibliotheksrasters (für die Weboberfläche)                |
| `GET` `POST`           | `/artists/{name}/image`                           | Eigenes Interpretenbild; Upload nur für Verwalter, multipart `image`        |
| `PUT` `DELETE`         | `/artists/{name}/cover`                           | Album als Interpretenbild wählen oder auf Zufall zurücksetzen (Verwalter)   |
| `GET`                  | `/search`                                         | Suche über die ganze Bibliothek                                             |
| `GET` `PATCH`          | `/tracks/{id}`                                    | Einzelner Track; Metadaten bearbeiten (Verwalter)                           |
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
| `POST`                 | `/podcasts/preview`                               | Feed ansehen, ohne zu abonnieren                                            |
| `POST`                 | `/podcasts`                                       | Feed abonnieren                                                             |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Aktualisieren / Abo beenden                                                 |
| `POST`                 | `/episodes/{id}/download`                         | Episode auf den Server laden                                                |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Scan steuern (Verwalter)                                                    |
| `GET` `POST`           | `/users`                                          | Benutzer auflisten / anlegen (Verwalter)                                    |
| `PATCH` `DELETE`       | `/users/{id}`                                     | Benutzer ändern / löschen (Verwalter)                                       |
| `GET`                  | `/server/info`                                    | Bestandszahlen, für Verwalter auch Benutzerzahl                             |

---

## Entwicklung

Du brauchst eine Rust-Toolchain (stable, Edition 2024), Docker für Postgres, ffmpeg und Node für
die JS-Tests. Das `Makefile` fasst die üblichen Schritte zusammen (`make help`):

```sh
make setup            # .env aus .env.example, dev-library/, Postgres auf Port 5434, erster Build
make dev              # cargo run auf http://localhost:3000
```

Ohne make:

```sh
cp .env.example .env
docker compose up -d postgres   # Postgres auf localhost:5434
cargo run                       # spielt die Migrationen ein, legt den ersten Verwalter an, lauscht auf :3000
```

Templates (askama) und Migrationen sind ins Binary kompiliert, eine Template-Änderung braucht
also einen Neustart; CSS und JS aus `static/` kommen direkt von der Platte.

### Tests

```sh
cargo test                      # Unit- und Integrationstests
cargo test --test integration   # nur Integrationstests: eine Wegwerf-Datenbank je Test auf :5434
node --test 'tests/js/*.test.mjs'  # JS-Module der Weboberfläche
make test                       # fmt, clippy, cargo test, JS-Tests
```

Die Integrationstests treiben den echten Router im Prozess gegen Postgres; `.cargo/config.toml`
zeigt sie auf die Compose-Datenbank, `docker compose up -d postgres` ist alles, was sie brauchen.

Die Playwright-End-to-End-Suite liegt in `e2e/`. Der Browser läuft dabei in einer macOS-VM auf
einem eigenen Build-Rechner (`e2e/run-on-jarvis.sh`), nicht auf dem Entwicklerrechner; wer sie
anderswo laufen lassen will, passt das Skript an.

### Prüfungen

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

### Docker-Image

```sh
docker build -t autoreverse-server .
```

Das `Dockerfile` cross-kompiliert in einer `rust:*-slim`-Stufe für die Zielarchitektur (kein QEMU
für den Rust-Build) und liefert das Binary mit `static/` auf `debian:trixie-slim` samt ffmpeg aus,
als UID 1000.

### Aufbau

```
src/
  api/          JSON-Endpunkte, ein Modul je Bereich
  web/          serverseitig gerenderte Weboberfläche (deutsch), ein Modul je Bereich
  auth/         Sitzungen, Tokens, Passwort-Hashing, Anmeldedrosselung, erster Verwalter
  library/      Abfragen, Fortschritt, Lesezeichen, Favoriten, Wiedergabelisten, Metadaten
  podcasts/     Feeds, Verzeichnissuche, Episoden-Downloads, Vorhalten, Hintergrundlauf
  scanner/      Hörbuch- und Musikscan, Tags, Kapitel, Cover
  streaming/    HTTP-Range-Antworten
  app.rs        der Router; main.rs die Kommandozeile
templates/      askama-Templates
static/         CSS, JS (htmx mitgeliefert), Icons
migrations/     sqlx-Migrationen
tests/          Integrationstests, JS-Tests, Fixtures
e2e/            Playwright-Specs
```

---

## Mitmachen

Issues und Pull Requests sind willkommen.

- Vor einem PR `make test` laufen lassen (oder `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `node --test 'tests/js/*.test.mjs'`).
- Schemaänderungen kommen als neue Datei nach `migrations/`; eine veröffentlichte Migration wird
  nie geändert.
- Die API wird von nativen Clients benutzt: Pfade, Feldnamen (snake_case) und Statuscodes
  kompatibel halten.
- Die Oberfläche ist deutsch; bitte Wortwahl und Ton der vorhandenen Texte treffen.

---

## Lizenz

Lizenziert unter der **GNU Affero General Public License v3.0**, siehe [LICENSE](LICENSE).

Die Netzwerk-Klausel der AGPL ist hier der springende Punkt: Wer eine veränderte Fassung von
Autoreverse betreibt und andere über ein Netzwerk darauf zugreifen lässt, muss ihnen auch den
veränderten Quellcode anbieten.
