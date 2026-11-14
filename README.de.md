# Autoreverse Server

Ein selbstgehosteter Medienserver für **Hörbücher, Musik und Podcasts** — mit JSON-API für native
Clients und einer Web-Oberfläche, die auch für sich allein genügt.

Autoreverse durchsucht deine vorhandenen Ordner mit Audiodateien, liest deren Tags, extrahiert
Kapitel und Cover und liefert alles über HTTP aus — inklusive Hörfortschritt, Lesezeichen,
Favoriten und Wiedergabelisten pro Benutzer. Podcasts abonnierst du per Feed, findest sie über
eine Verzeichnissuche, und einzelne Episoden lassen sich auf den Server herunterladen.

Der Server ist ein einzelnes Rust-Binary mit Postgres dahinter, ffmpeg (`ffprobe`, `ffmpeg`) ist
sein einziger Helfer von außen. Kein Transcoding-Daemon, kein Plugin-System, kein Medienserver-Wildwuchs — ein
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
- Wiedergabelisten mit sortierten Einträgen, Beschreibung, eigenem Cover und Ordnern; auf Wunsch
  für die anderen Konten des Servers sichtbar und von ihnen bearbeitbar; Export und Import als M3U
- Titel-Radio: ähnliche Titel aus der eigenen Sammlung zu einem Titel, einem Album oder einem
  Interpreten — gewählt aus Tags und dem eigenen Hörverlauf, ohne Sprachmodell und ohne fremden
  Dienst. Der Web-Player spielt damit weiter, wenn eine Liste zu Ende ist (Einstellungen →
  Wiedergabe → Autoplay)
- Lautstärke angleichen: Jeder Musiktitel bekommt einen Wert, um wie viel er bis −18 LUFS zu
  verstellen ist — aus seinen ReplayGain- oder R128-Tags oder vom Server im Hintergrund gemessen;
  die Clients regeln damit nach (Web-Player: Einstellungen → Wiedergabe → Lautstärke angleichen)
- Internetradio: öffentliche Sender in einer Liste je Konto, gefunden im freien Verzeichnis
  radio-browser.info oder per Adresse hinzugefügt, gehört über den Server mit laufendem Titel
- Fernsteuerung zwischen deinen Geräten: sehen, was wo läuft, es von einem anderen Gerät aus
  steuern, die Wiedergabe übernehmen („Geräte" im Web-Player; die Apps sprechen dieselben Routen)
- Wiedergabesitzungen: du siehst, was gerade anderswo läuft, und kannst es beenden
- Hörstatistik aus dem, was wirklich gehört wurde, auch offline Gehörtes, das später nachgemeldet wird
- Meistgehörte Titel, Alben, Interpreten und Genres, „On Repeat" und „Repeat Rewind" und ein
  Rückblick auf jedes Jahr — gezählt aus dem eigenen Hörverlauf, ohne Sprachmodell
- Songtexte aus den eigenen Dateien: eine `.lrc` oder `.txt` neben dem Titel oder in die Tags
  eingebetteter Text, im Web-Player läuft synchroner Text mit. Kein Songtext-Dienst wird gefragt

**Podcasts**

- Verzeichnissuche (öffentliche iTunes-Search-API von Apple) — nach Namen suchen statt Feed-URLs
  zusammensuchen
- Feed-Vorschau vor dem Abonnieren
- Die neuesten N Folgen eines Abos vorhalten — ein globaler Standard, pro Feed überschreibbar.
  Downloads jenseits dieser Zahl werden beim nächsten Aktualisieren wieder entfernt
- Aktualisierung je Feed, Episoden auf Wunsch in den Serverspeicher heruntergeladen

**Benutzer**

- Passwort-Hashing mit Argon2id
- Bearer-Token für API-Clients, Session-Cookies für die Weboberfläche, ein gemeinsamer Resolver;
  in der Datenbank liegt nur ein SHA-256 der Tokens, nach 180 Tagen ohne Nutzung verfallen sie
- Schutz gegen Anfragen fremder Seiten (`Origin`, `Sec-Fetch-Site`), Clickjacking-Schutz und eine
  Content-Security-Policy ohne Inline-Skripte
- Drosselung bei Fehlanmeldungen (`429` mit `Retry-After`), pro Name und pro öffentlicher
  Client-Adresse
- Dateien, Artwork und Kapitel, die ein Feed aus dem Internet nennt, lädt der Server nie aus dem
  lokalen Netz
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
| `AUTOREVERSE_DATA`                  | nein                  | `./data` (`/data` im Image)                                       | Basisverzeichnis für servereigene Dateien. `covers/`, `podcasts/`, `artists/` und `radio/` entstehen darin automatisch. |
| `AUTOREVERSE_ADMIN_USER`            | nur beim ersten Start | —                                                                 | Benutzername des ersten Verwalterkontos.                                                                      |
| `AUTOREVERSE_ADMIN_PASSWORD`        | nur beim ersten Start | —                                                                 | Passwort dieses Kontos.                                                                                       |
| `ORIGIN`                            | hinter einem Proxy    | aus dem `Host`-Header abgeleitet, Protokoll als `http` angenommen | Öffentliche URL des Servers. Siehe [Betrieb hinter einem Reverse Proxy](#betrieb-hinter-einem-reverse-proxy). |
| `AUTOREVERSE_RADIO_DIRECTORY_URL`   | nein                  | `https://all.api.radio-browser.info`                              | Basisadresse des Verzeichnisses: ein Name für alle seine Server. Bei Bedarf auf einen einzelnen Server (`https://de1.api.radio-browser.info`) oder einen Spiegel zeigen. |
| `HOST` / `PORT`                     | nein                  | `0.0.0.0` / `3000`                                                | Wo der Server lauscht.                                                                                        |
| `AUTOREVERSE_STATIC`                | nein                  | `static/` neben dem Binary, sonst `./static` (`/app/static` im Image) | CSS, JS und Icons. Findet der Server sie nicht, startet er nicht.                                         |
| `RUST_LOG`                          | nein                  | `autoreverse_server=info,tower_http=warn`                         | Log-Filter.                                                                                                   |

Alles andere stellt ein Verwalter zur Laufzeit unter Einstellungen → Server ein (oder über
[`PATCH /server/settings`](#server-einstellungen)), sofort wirksam: die Intervalle von
Bibliotheks-Scan, Podcast-Aktualisierung und Interpreten-Angaben, das Messen der Lautheit,
Internetradio und sein Verzeichnis, die Fernsteuerung und das Sprachmodell für Vorschläge. Frühere
Versionen lasen das aus `AUTOREVERSE_SCAN_HOURS`, `AUTOREVERSE_PODCAST_REFRESH_HOURS`,
`AUTOREVERSE_ARTIST_INFO`/`_DAYS`, `AUTOREVERSE_LOUDNESS`/`_THREADS`, `AUTOREVERSE_RADIO`/`_DIRECTORY`,
`AUTOREVERSE_DEVICES` und `AUTOREVERSE_AI_*`: Der erste Start dieser Version übernimmt sie einmal
und warnt bei späteren Starts im Log, solange sie noch gesetzt sind.

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

Unabhängig von `ORIGIN` lehnt der Server jede ändernde Anfrage ohne Bearer-Token ab, die der Browser
als von einer fremden Seite stammend kennzeichnet (`Sec-Fetch-Site`). Jede Antwort trägt
`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: same-origin` und eine
Content-Security-Policy; der Proxy muss sie nicht ergänzen. HSTS ist Sache des Proxys.

Die [Fernsteuerung](#fernsteuerung) hält je Gerät eine langlebige Antwort offen
(`GET /me/devices/events`, Server-Sent Events). Der Server schickt `X-Accel-Buffering: no`, damit
nginx sie nicht puffert, und alle 15 Sekunden eine Zeile – weit innerhalb der üblichen
Lese-Timeouts (nginx `proxy_read_timeout`: 60 s). Einem Proxy, der `text/event-stream` trotzdem
puffert oder komprimiert, muss man das für diesen Pfad abgewöhnen.

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

**ffmpeg** muss im `PATH` liegen: `ffprobe`, damit Kapitel aus `.m4b`-Dateien gelesen werden,
`ffmpeg`, um Musik zu dekodieren, deren Lautheit gemessen werden muss. Das mitgelieferte
Docker-Image installiert es; bei einer Installation ohne Container musst
du ffmpeg selbst bereitstellen und das Verzeichnis `static/` neben das Binary legen (oder
`AUTOREVERSE_STATIC` setzen). Ohne ffmpeg läuft alles andere weiter — `.m4b`-Dateien kommen dann
eben ohne Kapitel an, und Musik ohne Lautheits-Tags bleibt ohne Lautheitswert (der Server sagt
das im Log und versucht es nach dem nächsten Scan wieder).

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
| `GET`                  | `/me/history`                                     | Zuletzt Gehörtes — `?limit=`, siehe [Hörverlauf](#hörverlauf)               |
| `GET`                  | `/me/top`                                         | Meistgehörte Titel, Alben, Interpreten oder Genres, siehe [Meistgehört und Jahresrückblick](#meistgehört-und-jahresrückblick) |
| `GET`                  | `/me/on-repeat`, `/me/repeat-rewind`              | Zuletzt oft gehörte Titel; früher oft und seither nicht mehr gehörte        |
| `GET`                  | `/me/review`, `/me/review/years`                  | Ein Kalenderjahr Hörzeit; die Jahre, für die es eines gibt                  |
| `POST`                 | `/me/listens`                                     | Hörzeit nachmelden, die ohne Sitzung gemessen wurde, siehe [Hörzeit](#hörzeit) |
| `POST`                 | `/me/password`                                    | Eigenes Passwort ändern                                                     |
| `GET` `PUT`            | `/me/playback`                                    | Wiedergabeeinstellungen                                                     |
| `GET`                  | `/items`                                          | Titel auflisten — `?kind=book\|album\|podcast\|episode`, `?q=`, `?artist=`, `?genre=`, `?missing=` |
| `GET` `PATCH`          | `/items/{id}`                                     | Einzelner Titel; Metadaten bearbeiten (Verwalter)                           |
| `POST`                 | `/items/bulk`, `/items/bulk/undo`                 | Metadaten in Masse bearbeiten und zurücknehmen (Verwalter)                  |
| `POST`                 | `/items/series`                                   | Reihen und Bandnummern erkennen oder zuweisen, vorab als Probelauf (Verwalter) |
| `GET`                  | `/items/{id}/children`                            | Tracks bzw. Episoden eines Titels                                           |
| `GET`                  | `/items/{id}/cover`                               | Coverbild                                                                   |
| `DELETE`               | `/items/missing`                                  | Fehlende Titel entfernen (Verwalter)                                        |
| `GET`                  | `/artists`                                        | Interpreten                                                                 |
| `GET`                  | `/genres`                                         | Genres der Alben mit Albenzahl                                              |
| `GET`                  | `/library/more`                                   | Nächste Seite des Bibliotheksrasters (für die Weboberfläche)                |
| `GET` `POST`           | `/artists/{name}/image`                           | Eigenes Interpretenbild; Upload nur für Verwalter, multipart `image`        |
| `PUT` `DELETE`         | `/artists/{name}/cover`                           | Album als Interpretenbild wählen oder auf Zufall zurücksetzen (Verwalter)   |
| `GET`                  | `/search`                                         | Suche über die ganze Bibliothek, auch Titel nach Interpret und Interpreten; Titel mit dem Suchwort im Namen vor denen, die nur ihr Interpret findet |
| `GET` `PATCH`          | `/tracks/{id}`                                    | Einzelner Track; Metadaten bearbeiten (Verwalter)                           |
| `GET` `HEAD`           | `/tracks/{id}/stream`                             | Audiostream mit Range-Unterstützung                                         |
| `GET`                  | `/tracks/{id}/lyrics`                             | Songtext des Titels aus der Sammlung, siehe [Songtexte](#songtexte)         |
| `GET`                  | `/radio`                                          | Ähnliche Titel zu einem Titel, Album oder Interpreten, siehe [Titel-Radio](#titel-radio) |
| `GET` `POST`           | `/radio/stations`, `/radio/stations/search`       | Internet-Radiosender des Nutzers; Suche im Verzeichnis, siehe [Radiosender](#radiosender) |
| `PATCH` `DELETE`       | `/radio/stations/{id}`                            | Sender umbenennen, umsortieren, entfernen                                   |
| `GET` `HEAD`           | `/radio/stations/{id}/stream`, `…/now`, `…/logo`  | Live-Strom über den Server, laufender Titel, Logo                           |
| `GET`                  | `/radio/stations/search/logo?id=`                 | Logo eines Suchtreffers, vom Server geholt und ausgeliefert                 |
| `POST`                 | `/play/{itemId}`                                  | Wiedergabesitzung öffnen                                                    |
| `POST`                 | `/sessions/{sessionId}/close`                     | Wiedergabesitzung schließen: `{position?, listened?, tracks?}`, siehe [Hörzeit](#hörzeit) |
| `GET` `PUT/POST`       | `/progress/{itemId}`, `/progress/continue`        | Hörfortschritt und „Weiterhören"                                            |
| `GET` `POST` `DELETE`  | `/bookmarks`, `/bookmarks/{id}`                   | Lesezeichen                                                                 |
| `GET`                  | `/favorites`                                      | Favoriten                                                                   |
| `POST` `DELETE`        | `/favorites/items/{id}`, `/favorites/tracks/{id}` | Favoriten setzen/entfernen                                                  |
| `GET` `POST`           | `/playlists`                                      | Wiedergabelisten auflisten (`?scope=mine\|shared\|all`) / anlegen, siehe [Wiedergabelisten](#wiedergabelisten) |
| `GET` `PATCH` `DELETE` | `/playlists/{id}`                                 | Lesen / umbenennen, beschreiben, teilen / löschen                           |
| `POST` `PUT` `DELETE`  | `/playlists/{id}/entries[/{entryId}]`             | Einträge einer Wiedergabeliste                                              |
| `GET` `PUT` `DELETE`   | `/playlists/{id}/cover`                           | Das eigene Cover der Wiedergabeliste                                        |
| `GET`                  | `/playlists/{id}/export`                          | Die Wiedergabeliste als M3U-Datei (`?format=m3u8`) oder JSON (`?format=json`) |
| `POST`                 | `/playlists/import`                               | Eine neue Wiedergabeliste aus einem M3U-Text                                |
| `GET`                  | `/podcasts/search`                                | Verzeichnissuche nach Namen                                                 |
| `GET`                  | `/podcasts`, `/podcasts/new`                      | Abos mit Ungehört-Zahl; ungehörte Folgen aller Abos                         |
| `PUT`                  | `/podcasts/{id}/keep`                             | Folgen vorhalten: `{keep}`, `null` = Standard (Verwalter)                   |
| `POST`                 | `/podcasts/preview`                               | Feed ansehen, ohne zu abonnieren (Verwalter)                                |
| `POST`                 | `/podcasts`                                       | Feed abonnieren (Verwalter)                                                 |
| `POST` `DELETE`        | `/podcasts/{id}/refresh`, `/podcasts/{id}`        | Aktualisieren / Abo beenden (Verwalter)                                     |
| `POST`                 | `/episodes/{id}/download`                         | Episode auf den Server laden                                                |
| `POST` `GET`           | `/scan`, `/scan/status`, `/scan/cancel`           | Scan steuern (Verwalter); `loudness` meldet den Mess-Durchgang              |
| `GET` `POST`           | `/users`                                          | Benutzer auflisten / anlegen (Verwalter)                                    |
| `PATCH` `DELETE`       | `/users/{id}`                                     | Benutzer ändern / löschen (Verwalter)                                       |
| `GET` `DELETE`         | `/me/devices`, `/me/devices/events`, `/me/devices/{device_id}` | Die Online-Geräte des Kontos, der Ereignisstrom eines Geräts, Abmelden – siehe [Fernsteuerung](#fernsteuerung) |
| `PUT` `POST`           | `/me/devices/{device_id}/state`, `…/commands`, `…/transfer` | Melden, was läuft; ein anderes Gerät steuern; die Wiedergabe übertragen |
| `GET`                  | `/server/info`                                    | Bestandszahlen, für Verwalter auch Benutzerzahl; `radio`, `radio_directory`, `devices`; `web_url`, siehe [Als Link teilen](#als-link-teilen) |
| `GET` `PATCH`          | `/server/settings`                                | Verwalter: die Einstellungen des Servers, siehe [Server-Einstellungen](#server-einstellungen) |
| `GET`                  | `/ai/status`                                      | Ob ein Sprachmodell eingerichtet ist                                        |
| `GET` `POST`           | `/ai/feed`, `/ai/feed/refresh`                    | Der „Entdecken"-Feed, siehe [Entdecken-Feed](#entdecken-feed); seine KI-Teile neu erzeugen |
| `POST`                 | `/ai/playlist`, `/ai/suggestions`, `/ai/discover` | Playlist aus einem Satz (`{prompt}`), Albumvorschläge, neue Interpreten — brauchen ein Modell |

### Hörzeit

`/me/stats` und die Vorschläge unter „Entdecken" lesen aus demselben Buch der Hörzeiten. Zeit
kommt auf zwei Wegen hinein. Für ein Stück Wiedergabe genau einen davon benutzen, sonst zählt es
doppelt.

Alle Zeiten sind **Mediensekunden**: wie viel der Aufnahme zu hören war, nicht wie lange das
gedauert hat. Eine Stunde Hörbuch in doppeltem Tempo ist 3600, nicht 1800. Gezählt wird, was der
Player beim Abspielen tatsächlich durchlaufen hat — nicht der Abstand zweier Positionen, der nach
jedem Sprung falsch ist.

**Mit Sitzung** (`POST /play/{itemId}` … `POST /sessions/{sessionId}/close`). Der Body beim
Schließen:

| Feld       | Typ             | Pflicht | Bedeutung                                                              |
| ---------- | --------------- | ------- | ---------------------------------------------------------------------- |
| `position` | Zahl oder null  | nein    | Wo das Item jetzt steht, in Sekunden über seine Tracks. Wird als Fortschritt gespeichert. `null` oder weggelassen: der Fortschritt bleibt unberührt, siehe unten. |
| `listened` | Zahl oder null  | nein    | In dieser Sitzung gehörte Mediensekunden, vom Client gemessen.         |
| `tracks`   | Liste oder null | nein    | Wie sich diese Zeit auf die Titel des Items verteilt: `[{"track_id": Ganzzahl, "seconds": Zahl}]`. Damit kennt die Statistik die gehörten Lieder, siehe unten. |

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

- Mit `listened` wird genau das gebucht, begrenzt auf das Plausible: nicht unter 0, nicht über
  (Sekunden seit Öffnen der Sitzung + 60) × 4 (das höchste Tempo), nicht über 7 Tage. Ein
  gekürzter Wert ist trotzdem ein Erfolg. `0` bucht nichts.
- Ohne `listened` (oder mit `null`) bucht der Server wie bisher `position − start_position`, und
  nichts, wenn das negativ ist. So verhalten sich Clients, die das Feld nicht kennen. Das zählt
  einen Sprung nach vorn mit und verliert einen Sprung zurück; neue Clients sollten `listened`
  deshalb immer schicken.
- **Ohne `position`** (`null` oder weggelassen) wird die Sitzung geschlossen und `listened`
  gebucht, der gespeicherte Fortschritt des Items bleibt aber genau, wie er ist: Es wird kein
  Eintrag angelegt und keiner überschrieben, `finished` und der Platz unter `/progress/continue`
  ändern sich nicht. Das schickt ein Client nach einem **einzelnen Titel** aus einer Playlist,
  einem Mix oder dem Radio — ein Lied soll sein Album nicht unter „Weiterhören" bringen und
  nicht verschieben, wo das Album stehen geblieben ist. Für so eine Sitzung darf der Client auch
  kein `PUT /progress/{itemId}` schicken. Fehlen `position` und `listened` beide, wird die
  Sitzung geschlossen und nichts gebucht (die Ersatzrechnung oben braucht eine Position).
- **Mit `tracks`** wird die Zeit je Titel gebucht; daraus entstehen `/me/top`, `/me/on-repeat`
  und die meistgehörten Titel des Rückblicks. Jeder Eintrag sind die Mediensekunden, die von
  einem Titel **des Items dieser Sitzung** in dieser Sitzung zu hören waren; ein nicht gehörter
  Titel bleibt weg, ein in zwei Stücken gehörter darf einmal mit der Summe oder zweimal
  vorkommen. Eine Regel entscheidet, was gebucht wird, egal was sonst im Body steht:
  - Die Summe der Sitzung ist der **größere** Wert aus `listened` und der Summe der Einträge,
    begrenzt auf das Plausible, genau wie `listened` allein (siehe oben).
  - Jeder Eintrag wird auf seinen Titel gebucht. Liegt schon die Summe der Einträge über dieser
    Grenze, werden alle um denselben Faktor gekürzt, sodass sie zusammen die Grenze ergeben.
  - Was von der Summe nach den Einträgen übrig ist — `listened` war größer als ihre Summe —,
    wird **ohne Titel** auf das Item gebucht. Ist `listened` kleiner als die Summe oder fehlt
    es, bleibt nichts übrig und die Summe gilt.

  Also `listened` schicken wie bisher und `tracks` als seine Aufteilung; beides ergibt
  normalerweise dasselbe. Ein Eintrag wird ohne Fehler verworfen, wenn seine `track_id` kein
  Titel des Items der Sitzung ist (ein anderes Album, unbekannt, inzwischen gelöscht), wenn
  `track_id` keine ganze Zahl oder `seconds` keine Zahl über 0 ist. Bleibt kein Eintrag übrig —
  oder fehlt `tracks`, ist es `null` oder `[]` —, ist alles genau wie ohne das Feld, auch die
  Ersatzrechnung `position − start_position`. `tracks`, das keine Liste ist, ergibt `422`
  „tracks muss eine Liste sein", mehr als 5000 Einträge `422` „Höchstens 5000 Einträge in
  tracks"; die Sitzung bleibt offen. Die Summe des Items in `/me/stats`, `/me/history` und den
  Vorschlägen ist mit und ohne `tracks` dieselbe: Eine Sitzung mit drei Titeln sind drei Zeilen
  im Buch der Hörzeiten, und zusammen ergeben sie die eine Sitzung.
- `listened`, das keine Zahl ist, ergibt `422` `{"detail": "listened muss eine Zahl sein"}`, eine
  `position`, die da ist, aber keine Zahl (ein Text, eine Liste), `422` „position muss eine Zahl
  sein"; die Sitzung bleibt offen. Die übrigen Antworten: `204` bei Erfolg, `404` „Unbekannte
  Sitzung" für eine unbekannte, eine fremde oder eine schon geschlossene Sitzung. Unbekannte
  Felder werden ignoriert.

Das Öffnen einer Sitzung (`POST /play/{itemId}`) speichert nichts am Fortschritt: Es liest die
gespeicherte Position (und gibt sie als `start_position` heraus, `0` bei einem beendeten Item)
und hält die Sitzung selbst fest. Es entsteht kein Fortschrittseintrag, „Weiterhören" und
`sort=played` bleiben unberührt, bis ein `PUT /progress/{itemId}` oder ein Schließen **mit**
Position kommt.

**Ohne Sitzung** — offline Gehörtes, das später nachgereicht wird, Einzeltitel, Playlists:
`POST /me/listens`.

```json
{
  "listens": [
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a11", "item_id": 412, "seconds": 1830.5, "started_at": "2026-11-10T06:12:00Z" },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a12", "track_id": 9051, "seconds": 214, "started_at": "2026-11-12T17:40:03+01:00" }
  ]
}
```

| Feld         | Typ          | Pflicht          | Bedeutung                                                                |
| ------------ | ------------ | ---------------- | ------------------------------------------------------------------------ |
| `key`        | Zeichenkette | ja               | Der eigene Name des Clients für diesen Eintrag, 1–100 Zeichen, etwa eine UUID, vergeben beim Aufzeichnen. Derselbe Schlüssel wird je Benutzer einmal gebucht, wie oft er auch gesendet wird. |
| `item_id`    | Ganzzahl     | eines von beiden | Das Hörbuch, Album oder die Folge.                                       |
| `track_id`   | Ganzzahl     | eines von beiden | Ein einzelner Track. Die Zeit zählt für das Item, zu dem er gehört, und wird zusätzlich auf den Track gebucht, für die [Statistik je Titel](#meistgehört-und-jahresrückblick). Kommen beide, muss der Track zu diesem Item gehören. |
| `seconds`    | Zahl         | ja               | Gehörte Mediensekunden, größer als 0.                                    |
| `started_at` | Zeichenkette | ja               | Wann dieses Stück Hören begann, ISO-8601 mit Zeitzone oder `Z`; Sekundenbruchteile sind erlaubt. |

Höchstens 500 Einträge je Anfrage. Grenzen je Eintrag: `started_at` darf bis zu 365 Tage
zurückliegen und höchstens 5 Minuten vor der Uhr des Servers (für Clients, deren Uhr vorgeht);
`seconds` wird auf (Sekunden seit `started_at` + 300) × 4 und auf 24 Stunden gekürzt. Die Zeit
zählt am UTC-Tag von `started_at`: Was letzte Woche offline gehört wurde, steht in der Statistik
der letzten Woche.

Die Antwort ist `200` mit einem Ergebnis je Eintrag, in der gesendeten Reihenfolge:

```json
{
  "results": [
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a11", "status": "booked", "seconds": 1830.5 },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a12", "status": "duplicate" },
    { "key": "8b1f6c1e-6f0a-4c59-9d3e-2f6c1d0f7a13", "status": "rejected", "detail": "Unbekanntes Item" }
  ]
}
```

| `status`    | Bedeutung                                                                                |
| ----------- | ---------------------------------------------------------------------------------------- |
| `booked`    | Gezählt. `seconds` ist, was gebucht wurde — weniger als gesendet, wenn gekürzt wurde.    |
| `duplicate` | Ein Eintrag mit diesem Schlüssel wurde schon gebucht; nichts hat sich geändert.          |
| `rejected`  | Nicht buchbar; `detail` nennt den Grund (unbekanntes Item oder unbekannter Track, ein fehlendes oder falsches Feld, `started_at` in der Zukunft oder zu alt). `key` ist `null`, wenn der Eintrag keinen brauchbaren hatte. |

Jeder Status ist endgültig: Den Eintrag aus der lokalen Warteschlange nehmen, was auch
zurückkommt, und ihn nur behalten, wenn die Anfrage als ganze scheiterte (keine Verbindung,
`401`, `5xx`). Die Anfrage als ganze ist nur dann `422`, wenn der Body nicht
`{"listens": [...]}` ist („listens muss eine Liste sein") oder mehr als 500 Einträge hat.
`/me/listens` rührt die Hörposition nicht an; die speichert weiterhin `PUT /progress/{itemId}`.

### Hörverlauf

`GET /me/history?limit=` listet, was der anfragende Nutzer zuletzt gehört hat, das Neueste
zuerst. `limit` ist 1–200, Vorgabe 50; alles andere ergibt `422` „limit außerhalb des gültigen
Bereichs".

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

| Feld          | Typ    | Inhalt                                                                         |
| ------------- | ------ | ------------------------------------------------------------------------------ |
| `listened_at` | Text   | Wann der jüngste Teil dieses Eintrags gehört wurde, ISO-8601 in UTC, ganze Sekunden. |
| `seconds`     | Zahl   | In diesem Eintrag gehörte Mediensekunden, zusammengezählt.                     |
| `item`        | Objekt | Das Hörbuch, Album oder die Folge, in der Form, die `/items` listet (`ItemSummary`). |

Die Quelle ist das Buch der [Hörzeiten](#hörzeit); ein Eintrag erscheint also, sobald eine
Sitzung geschlossen oder eine Meldung eingereicht ist — nicht, solange etwas noch läuft.
Buchungen desselben Items, die aufeinander folgen, sind **ein** Eintrag (ein Album, über drei
Sitzungen gehört, ist eine Zeile: Sekunden addiert, der jüngste Zeitpunkt); dasselbe Item
erscheint erneut, wenn dazwischen etwas anderes gehört wurde. Items, deren Dateien fehlen,
bleiben weg, und was davor und danach gehört wurde, schließt sich zusammen. Der Verlauf ist **je
Item, nicht je Titel**: Eine Sitzung, die Titel für Titel gebucht wurde (`tracks` beim Schließen),
ist weiterhin ein Eintrag für ihr Album, ebenso was `/me/listens` mit einer `track_id` meldet.
Ein leerer Verlauf ist `{"entries": []}`. Der Verlauf reicht über die jüngsten 5000 Buchungen
zurück.

### Meistgehört und Jahresrückblick

Fünf Routen lesen das Buch der [Hörzeiten](#hörzeit) als Statistik zurück. Keine braucht ein
Sprachmodell; alle gelten je Nutzer und antworten ohne Anmeldung mit `401`.

**Was zählt.** Sekunden sind Mediensekunden, wie überall. Ein **Hörvorgang** (`plays`) ist ein
Hören von mindestens 30 Sekunden: bei einem Titel eine Buchung dieses Titels (eine Zeile je
Titel und Sitzung, oder ein Eintrag von `/me/listens`); bei einem Album oder Interpreten die
Buchungen eines Zeitpunkts zusammengezählt — ein Album, das in einer Sitzung lief, ist also
**ein** Hörvorgang, egal wie viele seiner Titel liefen, und eine Sitzung mit drei Liedern sind
nicht drei Hörvorgänge des Albums. Titel, Interpreten und Genres zählen **nur Musik** (Alben);
Zeit in Hörbüchern und Folgen steht in den Summen des Rückblicks und in keiner der Bestenlisten.
Hörzeit ohne Titel — alles aus der Zeit, bevor Clients `tracks` schickten, und Einträge von
`/me/listens` nur mit `item_id` — zählt für Alben, Interpreten und Genres, kann aber nicht unter
den Titeln erscheinen. Items, deren Dateien fehlen (`missing_since`), bleiben aus jeder Liste weg.

#### `GET /me/top`

| Parameter | Typ      | Bedeutung                                                                      |
| --------- | -------- | ------------------------------------------------------------------------------ |
| `kind`    | Text     | Pflicht: `tracks`, `albums`, `artists` oder `genres`.                          |
| `period`  | Text     | `week` (die letzten 7 Tage), `month` (30 Tage, der Standard), `year` (365 Tage) oder `all`. Gleitend, von jetzt zurückgerechnet. |
| `year`    | Ganzzahl | Ein Kalenderjahr in UTC, 2000–2100, z. B. `2026`. Ist es da, gilt es **statt** `period`. |
| `limit`   | Ganzzahl | 1–100, Standard 25.                                                            |

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

| Feld      | Typ            | Inhalt                                                                          |
| --------- | -------------- | ------------------------------------------------------------------------------- |
| `kind`    | Text           | Das angefragte `kind`.                                                          |
| `from`    | Text oder null | Beginn des Zeitraums, einschließlich. `null` bei `period=all`.                  |
| `to`      | Text           | Ende des Zeitraums, **ausschließlich**: der Zeitpunkt der Anfrage, bei `year=` die erste Sekunde des Folgejahres (`2027-01-01T00:00:00Z`), auch solange das Jahr noch läuft. |
| `entries` | Liste          | Das am längsten Gehörte zuerst (`seconds` absteigend, dann `plays`). Die Form hängt von `kind` ab, siehe unten. Leer, wenn nichts gehört wurde. |

| `kind`    | Ein Eintrag                                                                                 |
| --------- | ------------------------------------------------------------------------------------------- |
| `tracks`  | `{"track": TrackSummary, "seconds": Zahl, "plays": Ganzzahl}` — `TrackSummary` wie beim [Titel-Radio](#titel-radio). |
| `albums`  | `{"item": ItemSummary, "seconds": Zahl, "plays": Ganzzahl}` — `ItemSummary` wie in `/items`; immer `kind: "album"`. |
| `artists` | `{"artist": Text, "seconds": Zahl, "plays": Ganzzahl, "has_image": Wahrheitswert}`          |
| `genres`  | `{"genre": Text, "seconds": Zahl}` — ohne `plays`.                                          |

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

- **Interpreten.** Eine Buchung zählt für den eigenen Interpreten des Titels, wenn sie einen
  Titel nennt, der einen hat (die Lieder eines Samplers, ein Gast), sonst für den Interpreten des
  Albums. Schreibweisen, die sich nur in Groß- und Kleinschreibung unterscheiden, sind ein
  Interpret. `has_image` sagt, ob `GET /artists/{artist}/image` für genau diesen Text `artist`
  (URL-kodiert) ein Bild liefert — wahr für Album-Interpreten mit Cover oder hochgeladenem Bild,
  falsch für die meisten Interpreten, die nur an einzelnen Titeln stehen.
- **Genres** sind das Genre des Albums; Alben ohne Genre bleiben weg. Ohne Rücksicht auf
  Groß- und Kleinschreibung.

| Status | `detail`                                               | Wann                                    |
| ------ | ------------------------------------------------------ | --------------------------------------- |
| `422`  | „kind muss tracks, albums, artists oder genres sein"   | `kind` fehlt oder ist etwas anderes.    |
| `422`  | „period muss week, month, year oder all sein"          | Ein unbekanntes `period` (ohne `year`). |
| `422`  | „year außerhalb des gültigen Bereichs"                 | `year` keine ganze Zahl in 2000–2100.   |
| `422`  | „limit außerhalb des gültigen Bereichs"                | `limit` außerhalb 1–100.                |

#### `GET /me/on-repeat` und `GET /me/repeat-rewind`

Beide nehmen `?limit=` (1–100, Standard 30; sonst `422` „limit außerhalb des gültigen Bereichs")
und antworten mit Titeln in der Form von `GET /radio` — genauso abzuspielen, als Einzeltitel:

```json
{ "tracks": [ { "id": 9051, "title": "Hallogallo", "artist": null, "duration": 602.5,
                "item_id": 412, "item_title": "Neu!", "item_kind": "album", "has_lyrics": false } ] }
```

- **`/me/on-repeat`**: die Titel mit mindestens **2 Hörvorgängen in den letzten 30 Tagen**, die
  häufigsten zuerst (bei Gleichstand: mehr Sekunden zuerst).
- **`/me/repeat-rewind`**: die Titel mit mindestens 2 Hörvorgängen, die **länger als 30 Tage
  zurückliegen**, und die in den letzten 30 Tagen gar nicht liefen — auch nicht für ein paar
  Sekunden —, die häufigsten zuerst.

Nur Musik, nichts von fehlenden Alben, kein Titel doppelt. **Eine leere Liste ist eine gültige
Antwort** (`{"tracks": []}`): ein neuer Nutzer, oder einer, dessen Clients noch keine Titel melden.

#### `GET /me/review` und `GET /me/review/years`

`GET /me/review?year=2026` ist ein **Kalenderjahr in UTC**; ohne `year` das laufende. Ein Jahr,
in dem nichts gehört wurde, ist kein `404`: Es kommt mit Nullen, `null` und leeren Listen zurück,
genau in der Form unten. `year` außerhalb 2000–2100 ist `422` „year außerhalb des gültigen
Bereichs".

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

| Feld                  | Typ              | Inhalt                                                             |
| --------------------- | ---------------- | ------------------------------------------------------------------ |
| `year`                | Ganzzahl         | Das Jahr, um das es geht.                                          |
| `seconds`             | Objekt           | `total`, `music` (Alben), `books`, `podcasts` (Folgen) — Zahlen; `total` ist die Summe der drei. Enthält auch Zeit in Items, die inzwischen fehlen. |
| `days_listened`       | Ganzzahl         | UTC-Tage, an denen irgendetwas gehört wurde.                       |
| `longest_day`         | Objekt oder null | `{"date": "JJJJ-MM-TT", "seconds": Zahl}` — der UTC-Tag mit der meisten Hörzeit, bei Gleichstand der früheste. `null` in einem leeren Jahr. |
| `longest_streak_days` | Ganzzahl         | Die meisten aufeinanderfolgenden UTC-Tage mit Hörzeit, innerhalb dieses Jahres. |
| `months`              | Liste            | Immer 12 Zahlen: Sekunden je Monat, Januar zuerst.                 |
| `top_tracks`, `top_albums`, `top_artists`, `top_genres` | Listen | Je bis zu 5 Einträge, genau die Einträge von `/me/top?kind=…&year=…`. |
| `books_finished`      | Liste            | `ItemSummary` jedes in diesem Jahr beendeten Hörbuchs, in der Reihenfolge des Beendens. Das Datum ist der Moment, in dem es als beendet markiert wurde; ein erneutes Speichern als beendet verschiebt es nicht. Ein seither neu begonnenes Buch ist nicht beendet und steht nicht drin. (Vorher beendete Bücher zählen nach dem letzten Speichern ihrer Position.) |
| `new_artists`         | Ganzzahl         | Interpreten (wie in `/me/top?kind=artists`), die in diesem Jahr zum allerersten Mal gehört wurden. |
| `first_listen`        | Objekt oder null | `{"at": ISO-Zeitpunkt, "item": ItemSummary}` — das Erste, was im Jahr gehört wurde; ein Item jeder Art. `null` in einem leeren Jahr. |

`GET /me/review/years` listet die Kalenderjahre (UTC), in denen etwas gehört wurde, das neueste
zuerst — das, was eine Jahresauswahl anbietet. `{"years": []}` bei einem neuen Nutzer.

```json
{ "years": [2026, 2025] }
```

### Songtexte

Songtexte kommen **aus der Sammlung selbst** — der Server fragt keinen Songtext-Dienst und
schickt keinen Titelnamen irgendwohin. Quellen, die erste mit einem Text gewinnt:

1. Eine Datei neben der Audiodatei mit gleichem Namen und der Endung `.lrc` (mit Zeiten) oder
   `.txt` (ohne); `.lrc` geht vor `.txt`. `01 Hallogallo.flac` → `01 Hallogallo.lrc`.
2. In die Audiodatei eingebetteter Text: ID3v2-Frames `USLT` (ohne Zeiten) und `SYLT` (mit
   Zeiten) sowie freie `TXXX`-Frames namens `LYRICS`, `UNSYNCEDLYRICS`, `SYNCEDLYRICS` oder
   `lyrics-<Sprache>` (`SYLT` und `TXXX` in MP3-Dateien); die Vorbis-Kommentare `LYRICS`, `UNSYNCEDLYRICS`, `SYNCEDLYRICS` (FLAC,
   Ogg, Opus); MP4 `©lyr`. Eingebetteter Text, der selbst LRC-Zeitstempel enthält, zählt als
   synchron, und Text mit Zeiten geht vor Text ohne.

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

| Feld     | Typ            | Inhalt                                                                          |
| -------- | -------------- | ------------------------------------------------------------------------------- |
| `synced` | Wahrheitswert  | `true`: jede Zeile hat eine Zeit. `false`: keine Zeile hat eine.                |
| `source` | Text           | `file` (eine `.lrc`/`.txt` neben dem Titel) oder `tags` (eingebettet).          |
| `lines`  | Liste          | In Lesereihenfolge; bei synchronem Text in Spielreihenfolge (`t` aufsteigend). Nie leer. |
| `t`      | Zahl oder null | Sekunden ab Beginn **des Titels** (nicht des Albums), zu denen die Zeile beginnt; Nachkommastellen sind üblich. `null` bei unsynchronem Text. |
| `text`   | Text           | Eine Zeile, ohne Zeilenumbruch. `""` ist auch eine Zeile: der Abstand zwischen Strophen, bei synchronem Text eine Instrumentalstelle — beibehalten. |

Bei synchronem Text ist die aktuelle Zeile die letzte, deren `t` ≤ der Position im Titel ist;
vor dem ersten `t` ist keine Zeile aktuell. Mehrere Zeilen können dasselbe `t` haben (eine Zeile
und ihre Übersetzung). Was der Server mit LRC macht: Eine Zeile mit mehreren Zeitstempeln
(`[00:12.00][01:15.00]`) wird eine Zeile je Stempel; Kopfzeilen (`[ar:…]`, `[ti:…]`,
`[length:…]`) fallen weg; `[offset:±ms]` wird angewendet (positiv: früher), nie unter 0;
Wortstempel aus „Enhanced LRC" (`<00:12.34>`) werden entfernt; Text ohne Stempel bekommt die
Zeit der Zeile davor. Stempel sind `[mm:ss]`, `[mm:ss.x]` bis `[mm:ss.xxx]`, mit `.`, `:` oder
`,` vor dem Bruchteil.

| Status | `detail`                  | Wann                                                         |
| ------ | ------------------------- | ------------------------------------------------------------ |
| `404`  | „Kein Songtext vorhanden" | Den Titel gibt es, er hat keinen Text.                       |
| `404`  | „Unbekannter Track"       | Den Titel gibt es nicht.                                     |

Grenzen: Ein Text über 200 kB (Datei oder eingebettet) gilt nicht als Songtext. Textdateien
werden als UTF-8 (mit oder ohne BOM), UTF-16 mit BOM oder sonst Latin-1 gelesen; Tags in der
Kodierung, die ihr Frame nennt. Gelesen wird nur die Datei neben dem Titel — ein Link, der aus
dem Ordner des Titels hinausführt, wird übergangen.

**`has_lyrics`.** Jede Titel-Ausgabe — die `tracks` von `POST /play/{itemId}` und von
`GET /items/{id}`, `GET /tracks/{id}` und jede `TrackSummary` (Suche, Favoriten, Playlists,
Titel-Radio, die Listen oben) sowie die `tracks` von `POST /ai/playlist` und der
`mixes` in `GET /ai/feed` — trägt `has_lyrics: Wahrheitswert`, damit ein Client seinen
Songtext-Knopf nur zeigt, wo er irgendwohin führt. Das Kennzeichen pflegt der Bibliotheks-Scan:
Jeder Scan (der automatische im Abstand aus Einstellungen → Server oder ein von Hand gestarteter) sucht
die Dateien neben jedem Titel, auch in Ordnern, die er sonst als unverändert überspringt, und
liest die Tags einer Datei einmal je Fassung der Datei nach Songtexten. Eine Mediathek aus der
Zeit vor dieser Funktion bekommt ihre Kennzeichen also mit dem ersten Scan nach dem Update, ohne
dass jemand etwas tun muss; dieser Scan liest einmal die Tags aller Titel und dauert entsprechend
länger. Zwischen zwei Scans kann das Kennzeichen den Dateien nachhinken — `GET
/tracks/{id}/lyrics` liest immer die Dateien dieses Augenblicks und berichtigt das Kennzeichen
seines Titels, wenn beide nicht übereinstimmen. Clients, die das Feld nicht kennen, übergehen es.

### Lautstärke

„Lautstärke angleichen": Der Server weiß, wie laut jeder Musiktitel ist, der Client regelt nach.
Jede Titel-Ausgabe — die `tracks` von `POST /play/{itemId}` und von `GET /items/{id}`,
`GET /tracks/{id}` und jede `TrackSummary` (Suche, Favoriten, Playlists, Titel-Radio, On Repeat,
die Top-Listen) — trägt drei weitere Felder. Jedes ist `null`, solange kein Wert bekannt ist;
Clients, die sie nicht kennen, übergehen sie.

| Feld            | Typ             | Inhalt                                                                 |
| --------------- | --------------- | ---------------------------------------------------------------------- |
| `gain_db`       | Zahl oder null  | Um wie viele dB der Titel zu verstellen ist, um **−18 LUFS** zu erreichen (die Referenz von ReplayGain 2). Negativ: leiser. |
| `album_gain_db` | Zahl oder null  | Dasselbe für das Album als Ganzes — ein Wert auf allen seinen Titeln, damit leise und laute Stücke ihr Verhältnis behalten. |
| `peak`          | Zahl oder null  | Der höchste Abtastwert des Titels, linear (`1.0` = Vollaussteuerung; verlustbehaftete Dateien liegen manchmal knapp darüber). |

```json
{ "id": 9051, "position": 1, "title": "Hallogallo", "duration": 602.5, "artist": null, "disc": 1,
  "has_lyrics": false, "gain_db": -6.48, "album_gain_db": -5.1, "peak": 0.988553 }
```

**Anwenden.** Die Abtastwerte mit `10^(gain/20)` multiplizieren. `album_gain_db` nehmen, solange
ein Album am Stück läuft (kein Sprung zwischen seinen Titeln), `gain_db` für einen einzelnen
Titel (Playlist, Warteschlange, Titel-Radio); ist nur einer der beiden bekannt, diesen. Ein
positiver Wert hebt an: den Faktor so begrenzen, dass `Faktor × peak ≤ 1` bleibt, und ohne
`peak` gar nicht anheben. Gesprochenes und Radiosender bleiben unberührt.

**Woher die Werte kommen**, in dieser Rangfolge:

1. Tags, die schon in der Datei stehen, gelesen beim Scan: `REPLAYGAIN_TRACK_GAIN`,
   `REPLAYGAIN_ALBUM_GAIN`, `REPLAYGAIN_TRACK_PEAK` (Vorbis-Kommentare in FLAC/Ogg, ID3 `TXXX`,
   APEv2 in MP3, MP4 `----:com.apple.iTunes:…`), wobei `REPLAYGAIN_REFERENCE_LOUDNESS`
   berücksichtigt wird, wenn es eine andere Referenz nennt; bei Opus `R128_TRACK_GAIN` und
   `R128_ALBUM_GAIN` (Q7.8, bezogen auf −23 LUFS, um +5 dB umgerechnet). `iTunNORM` wird nicht
   gelesen: Es lässt sich nicht verlässlich deuten.
2. Vom Server gemessen, wo die Tags etwas offenlassen: integrierte Lautheit nach EBU R128 und
   der Spitzenpegel, indem die Datei einmal mit `ffmpeg` dekodiert wird. Der Album-Wert ist die
   Lautheit über alle Titel des Albums zusammen (mit R128-Gating), nicht der Mittelwert ihrer
   Werte.

Gemessen wird nur **Musik**. Hörbücher und Podcast-Folgen behalten `null`, sofern ihre Dateien
keine Tags tragen — Hunderte Stunden Sprache zu dekodieren, brächte nichts.

**Der Mess-Durchgang** läuft im Hintergrund nach jedem Scan und bei jedem Start, Album für
Album, so viele Alben gleichzeitig wie unter Einstellungen → Server eingestellt (Standard: halbe Kernzahl, höchstens
4). Er ist wiederaufnehmbar — ein Album wird in einem Zug geschrieben, ein Neustart macht mit
den noch offenen Alben weiter — und endet mit dem Server. Eine Datei, die sich nicht dekodieren
lässt, steht im Log, bleibt `null` und wird nicht erneut versucht, bis sie sich ändert (Größe
oder Änderungszeit); dieselbe Änderung lässt jede Datei ihre Werte verlieren und neue bekommen.
Ebenso ein Album, das einen Titel verliert oder hinzubekommt (Datei gelöscht, zwischen Ordnern
verschoben, ein CD-Ordner jetzt als Teil seines Albums gelesen): Seine Titel werden neu gelesen
und gemessen und behalten bis dahin ihre alten Werte. Ein Album, dessen Dateien gerade nicht erreichbar sind, wird zurückgestellt. `GET /scan/status`
meldet den Durchgang als `"loudness": {"running", "total", "processed", "failed"}`
(`total`/`processed` in Alben, `failed` in Dateien); die Scan-Karte in den Web-Einstellungen
zeigt dasselbe. Unter Einstellungen → Server abgeschaltet wird nichts gemessen; Tags werden trotzdem gelesen.

**Eine vorhandene Mediathek** braucht kein Zutun: Nach dem Update liest der Server einmal die
Lautheits-Tags aller Titel (beim Start und bei jedem Scan) und misst den Rest im Hintergrund.
Das Dekodieren braucht auf einem Desktop-Prozessor grob eine Zehntelsekunde je Minute Audio und
Thread — ein Vier-Minuten-Titel etwa 0,45 s, 20.000 Titel also rund 40 Minuten mit vier Threads
und ein paar Stunden auf einem kleinen NAS. Bis ein Titel an der Reihe war, sind seine Felder
`null`, und er läuft unverändert.

**Der Web-Player** regelt über die `volume` des Audio-Elements, die bei 1,0 endet: Er stellt
laute Musik leiser und hebt nie an. Das Ziel von −18 LUFS liegt so tief, dass fast alle Musik
abgesenkt wird; eine leisere Aufnahme läuft, wie sie ist. Safari unter iOS beachtet `volume`
nicht, dort wird nichts angeglichen.

### Titel-Radio

In den Oberflächen heißt das „Titel-Radio" („Titel-Radio starten"), zur Unterscheidung von den
Internet-[Radiosendern](#radiosender). `GET /radio` antwortet mit Titeln aus der Sammlung, die zu einer Saat passen: das, was ein Client
in die Warteschlange legt, wenn jemand „mehr davon" möchte, oder womit er weiterspielt, wenn die
Warteschlange leer ist. Es braucht kein Sprachmodell und fragt keinen fremden Dienst.

| Parameter  | Typ      | Bedeutung                                                                     |
| ---------- | -------- | ----------------------------------------------------------------------------- |
| `track_id` | Ganzzahl | Saat: ein Titel. Die Antwort enthält diesen Titel nie.                        |
| `item_id`  | Ganzzahl | Saat: ein Album. Die Antwort enthält keinen Titel dieses Albums.              |
| `artist`   | Text     | Saat: ein Interpret nach Namen (Album-Interpret, Interpret oder der eigene Interpret eines Titels; Groß- und Kleinschreibung egal). Die Antwort enthält diesen Interpreten durchaus. |
| `limit`    | Ganzzahl | Wie viele Titel höchstens, 1–200, Vorgabe 50.                                 |
| `exclude`  | Ganzzahl | Eine Titel-ID, die wegbleiben soll — was der Client schon in der Warteschlange hat oder gerade gespielt hat. Den Parameter wiederholen, höchstens 500-mal. |

Genau einer von `track_id`, `item_id`, `artist`.

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

Jeder Eintrag ist das `TrackSummary`, das auch `/search` und die Playlists liefern:

| Feld         | Typ             | Inhalt                                                               |
| ------------ | --------------- | -------------------------------------------------------------------- |
| `id`         | Ganzzahl        | Titel-ID — für `/tracks/{id}/stream`.                                |
| `title`      | Text oder null  | Der Titel des Tracks; `null`, wenn seine Tags keinen haben.          |
| `artist`     | Text oder null  | Der **eigene** Interpret des Titels aus seinen Tags; `null`, wenn er keinen hat (dann gilt der des Albums, siehe `/items/{item_id}`). |
| `duration`   | Zahl            | Sekunden.                                                            |
| `item_id`    | Ganzzahl        | Das Album — für `/items/{id}/cover` und `/play/{id}`.                |
| `item_title` | Text            | Der Titel des Albums.                                                |
| `item_kind`  | Text            | Hier immer `album`.                                                  |
| `has_lyrics` | Wahrheitswert   | `GET /tracks/{id}/lyrics` hat einen Text dafür, siehe [Songtexte](#songtexte). |
| `gain_db`, `album_gain_db`, `peak` | Zahl oder null | Zum Angleichen der Lautstärke, siehe [Lautstärke](#lautstärke). |

Was für die Antwort immer gilt: nur Musik; nichts aus Alben, deren Dateien fehlen oder die als
Duplikat ausgeblendet sind; keiner der `exclude`-Titel; nicht der Saat-Titel; kein Titel doppelt.
Sie kann kürzer sein als `limit`, und **eine leere Liste ist eine gültige Antwort**
(`{"tracks": []}`) — eine kleine Sammlung, oder alles ausgeschlossen.

| Status | `detail`                                                                      | Wann                                     |
| ------ | ----------------------------------------------------------------------------- | ---------------------------------------- |
| `404`  | „Unbekannter Track" / „Unbekanntes Item" / „Interpret nicht gefunden"        | Die Saat gibt es nicht (ein Interpret: kein vorhandenes Album und kein Titel unter diesem Namen). |
| `422`  | „Genau einer der Parameter track_id, item_id oder artist muss angegeben sein" | Keine Saat, mehr als eine, oder ein leerer `artist`. |
| `422`  | „Titel-Radio gibt es nur für Musik"                                           | Die Saat ist ein Hörbuch, ein Podcast oder eine Folge, oder ein Track davon. |
| `422`  | „track_id muss eine ganze Zahl sein" (ebenso `item_id`, `exclude`)            | Keine ganze Zahl.                        |
| `422`  | „limit außerhalb des gültigen Bereichs"                                       | `limit` außerhalb 1–200.                 |
| `422`  | „Höchstens 500 exclude-Werte je Anfrage"                                      | Mehr als 500 `exclude`.                  |

Wie es auswählt. Jedes vorhandene Album wird an der Saat gemessen: Ein gemeinsames Genre zählt am
meisten (ein Tag wie `Rock; Pop` nennt zwei; `Indie Rock` und `Rock` zählen halb), dann der
Interpret der Saat (auch als Interpret eines Titels auf einem Sampler), dann das Jahr (voll
innerhalb von 3 Jahren, nichts mehr ab 15). Dazu kommt der Geschmack des anfragenden Nutzers —
als Favorit markierte Alben und Titel und Alben, die fünf Minuten oder länger gehört wurden,
wiegen etwas mehr — und ein Anteil Zufall, damit zwei Aufrufe verschiedene Listen liefern. Die
besten Alben geben reihum je einen Titel ab, die nächsten zuerst: Ein Radio beginnt nahe an
seiner Saat und öffnet sich dann. Kein Album gibt mehr als 3 Titel; der Interpret der Saat
bekommt neben einer Titel- oder Album-Saat höchstens ein Achtel der Liste (ein Drittel bei einer
`artist`-Saat), jeder andere Interpret ebenso — es sei denn, die Liste bliebe sonst zu kurz. Kein
Interpret spielt dreimal hintereinander, solange noch ein anderer da ist, der dazwischen passt.
Eine Interpreten-Saat steht für die Genres der Alben dieses Interpreten und deren mittleres Jahr.
Ohne Genre-Tags arbeitet dasselbe mit Interpret, Jahr, Geschmack und Zufall. Wie groß die
Sammlung auch ist, das kostet zwei Abfragen.

Um einen Radio-Titel zu spielen, ohne den Fortschritt seines Albums anzufassen: eine Sitzung für
`item_id` öffnen, den Titel streamen und mit `"position": null` schließen (siehe
[Hörzeit](#hörzeit)) — oder ihn ohne Sitzung spielen und über `POST /me/listens` mit seiner
`track_id` melden.

### Radiosender

Internetradio: Jedes Konto führt seine eigene Liste öffentlicher Sender und hört sie **über
diesen Server**. Ein Gerät spricht nur mit dem Server — eine Adresse, eine Anmeldung —, und der
Server liest den laufenden Titel aus dem Strom. `GET /server/info` nennt, ob es das Feature gibt
(`radio`) und ob Sender gesucht werden können (`radio_directory`); ist Radio unter
Einstellungen → Server abgeschaltet, antwortet jede Route unten mit `404` „Radio ist auf diesem Server abgeschaltet".

Ein Sender auf der Leitung:

```json
{ "id": 12, "name": "Jazz Eins", "homepage": "https://sender.example/", "country": "DE",
  "tags": ["jazz", "smooth jazz"], "codec": "MP3", "bitrate": 192, "has_logo": true }
```

`homepage`, `country` (ISO 3166-1 alpha-2), `codec` und `bitrate` (kbit/s) sind `null`, wo
unbekannt. Die Adresse des Stroms selbst geht nie an einen Client.

| Methode und Pfad                        | Antwort                                                                                  |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| `GET /radio/stations`                   | `200 {"stations": [Station…]}` in der Reihenfolge des Nutzers.                           |
| `GET /radio/stations/search?q=&country=&limit=` | `200 {"results": […]}` aus dem Verzeichnis: `q` 1–100 Zeichen, `country` zwei Buchstaben (optional), `limit` 1–50 (Standard 25). Jeder Treffer ist ein Sender ohne `id`, mit `directory_id`, `has_logo` (nach dem Logo zu fragen lohnt sich: Das Verzeichnis nennt eines oder eine Webseite, deren Symbol einspringen kann — anzuzeigen über die Route unten, die trotzdem `404` antworten kann), `logo_url` (die Adresse des Verzeichnisses, kann `null` sein; bleibt für ältere Clients, nicht laden) und `added` (der Nutzer hat ihn schon). Vom Verzeichnis als kaputt gemeldete Sender und HLS-Sender fehlen. `404` „Die Sendersuche ist auf diesem Server abgeschaltet" bei abgeschaltetem Verzeichnis (Einstellungen → Server), `502`, wenn das Verzeichnis nicht erreichbar ist. |
| `GET /radio/stations/search/logo?id=&size=` | Das Logo eines Suchtreffers: `id` ist dessen `directory_id`, `size` wie bei Covern. `200` mit dem Bild, sonst `404` „Kein Logo", siehe unten. |
| `POST /radio/stations`                  | Body `{"directory_id": "…"}` (Daten und Logo aus dem Verzeichnis) oder `{"stream_url": "…", "name": "…", "homepage": "…"?}` (das Logo ist dann das Symbol dieser Webseite, wenn sie eines hat). `201 Station`; `200` mit dem vorhandenen Sender, wenn der Nutzer ihn schon hat (gleiche `directory_id`, gleiche Adresse oder derselbe Strom hinter einer Playlist-Datei). |
| `PATCH /radio/stations/{id}`            | Body `{"name"?: "…", "position"?: n}` — umbenennen, verschieben (ab 1, hinter dem Ende ist das Ende; die Positionen bleiben lückenlos). `200 Station`. |
| `DELETE /radio/stations/{id}`           | `204`.                                                                                   |
| `GET /radio/stations/{id}/stream`       | Der Live-Strom, siehe unten.                                                             |
| `GET /radio/stations/{id}/now`          | `200 {"title", "artist", "raw", "listening"}`, siehe unten.                              |
| `GET /radio/stations/{id}/logo?size=`   | Das gespeicherte Logo (`size` wie bei Covern; verkleinert bleibt ein durchsichtiges PNG), `404` „Kein Logo" ohne. |

Jede `{id}`-Route antwortet mit `404` „Unbekannter Sender" — für eine unbekannte id ebenso wie
für den Sender eines anderen Nutzers.

**Hinzufügen.** Bevor ein Sender gespeichert wird, ruft der Server die Adresse selbst ab: Sie
muss `http(s)` sein, erreichbar, und Audio liefern. Playlist-Dateien (`.m3u`, `.pls`) werden bis
zum Strom verfolgt, den sie nennen. Sonst `422` mit einem deutschen Satz für den Nutzer —
darunter „Die Adresse muss mit http:// oder https:// beginnen.", „Der Sender ist unter dieser
Adresse nicht erreichbar.", „Unter dieser Adresse gibt es keinen Audio-Stream. …", „Adressen im
lokalen Netz können nicht als Sender hinzugefügt werden." und, bei HLS, „HLS-Sender (.m3u8)
werden noch nicht unterstützt. Bitte die MP3- oder AAC-Adresse des Senders verwenden.". Eine
unbekannte `directory_id` ist `404`, ein nicht erreichbares Verzeichnis `502`. Eine Liste fasst
höchstens 200 Sender.

**HLS wird (vorerst) nicht unterstützt:** Eine `.m3u8` ist kein Strom, sondern eine Playlist
immer neuer Segmentdateien, die der Server umschreiben und auf Zuruf eines Clients abrufen
müsste. Solche Sender werden beim Hinzufügen abgelehnt und aus der Suche gefiltert. Clients
bekommen deshalb immer einen durchgehenden MP3-, AAC- oder Ogg-Strom, den iOS, Android, Qt und
Browser von sich aus spielen.

**Der Strom.** `GET /radio/stations/{id}/stream` ist authentifiziert wie `/tracks/{id}/stream`
(Bearer-Token oder Sitzungs-Cookie) und reicht das Audio des Senders durch, wie es ankommt:
`200`, der `Content-Type` des Senders, kein `Content-Length`, `Accept-Ranges: none` (ein
`Range`-Header wird ignoriert), `Cache-Control: no-store`. Der Server fordert beim Sender
ICY-Metadaten an, nimmt sie aus dem Strom heraus — der Client bekommt reines Audio — und merkt
sich den Titel. Auch alte SHOUTcast-v1-Sender, die mit `ICY 200 OK` statt `HTTP/1.x 200 OK`
antworten, werden verstanden, beim Hinzufügen wie beim Abspielen; für den Client sehen sie aus
wie jeder andere. Schließt der Client die Verbindung, endet auch die zum Sender. `HEAD` antwortet
mit den Kopfzeilen, ohne den Sender zu rufen. `502` „Sender nicht erreichbar", wenn der Sender
nicht binnen zehn Sekunden antwortet, mit einem Fehler antwortet oder kein Audio mehr sendet;
`429` „Zu viele Radio-Streams gleichzeitig" (mit `Retry-After`) bei mehr als vier Strömen je
Nutzer zugleich. Schweigt ein Sender 20 Sekunden lang, endet der Strom; Clients sollten sich
dann neu verbinden.

**Läuft gerade.** `GET /radio/stations/{id}/now` nennt den zuletzt aus dem Strom gelesenen Titel:

```json
{ "title": "Autobahn", "artist": "Kraftwerk", "raw": "Kraftwerk - Autobahn", "listening": true }
```

`raw` ist der Text des Senders. `artist` und `title` sind nur gesetzt, wo er sich ohne Raten
zerlegen lässt — genau ein „ - " —, sonst `null` („Nachrichten", „A - B - C"): dann `raw`
zeigen. `listening` sagt, ob gerade ein Strom dieses Senders über den Server läuft. Ohne einen
solchen sind alle drei `null`: Der Server hört nicht von sich aus hinein, um das zu beantworten.
Alle 10–15 Sekunden fragen, **während gehört wird**; der erste Titel erscheint einen Moment nach
dem Start des Stroms. Sender ohne ICY-Metadaten (viele Ogg-Ströme) haben nie einen.

**Hörzeit.** Radio wird nicht gebucht: keine Sitzung, kein Fortschritt, nichts unter
„Weiterhören" und nichts in `/me/stats` oder `/me/history` — das Hörzeit-Buch zählt Titel der
Sammlung, und ein Live-Strom ist keiner. Clients rufen für einen Sender weder `/play` noch
`/progress` noch `/me/listens` auf.

**Logos.** Clients laden kein Logo von Dritten: Das eines gespeicherten Senders kommt von
`GET /radio/stations/{id}/logo`, das eines Suchtreffers von
`GET /radio/stations/search/logo?id=<directory_id>` — abzurufen, wo ein Treffer `has_logo: true`
meldet, erst beim Anzeigen, mit der `size`, die die Zeile braucht, und mit einem Platzhalter bei
`404`. **`has_logo` am Treffer heißt „es lohnt sich zu fragen", nicht „es gibt eines"**: Das
Verzeichnis nennt ein Logo oder eine Webseite, und von beidem ist nichts abgerufen, wenn die
Suche antwortet — die Suche wartet nie auf fremde Server. `404` „Kein Logo" ist die Antwort,
wann immer sich kein Logo zeigen lässt (keines zu finden, sein Server nicht erreichbar oder
langsamer als ein paar Sekunden, größer als 2 MB, kein Bild). Die Route nimmt eine id und nie
eine Adresse; der Server holt, was das Verzeichnis zu dieser id nennt, mit demselben Zaun gegen
das lokale Netz wie bei den Strömen.

Nennt das Verzeichnis kein Logo oder eines, das sich nicht laden lässt (beides ist häufig: Das
Feld pflegen Freiwillige), liest der Server die Webseite des Senders und nimmt deren Symbol:
zuerst ein `apple-touch-icon`, dann `rel="icon"` nach angegebener Größe, dann
`/apple-touch-icon.png` und `/favicon.ico`, zuletzt `og:image` (oft ein Foto statt eines
Logos). Ein Bild mit weniger als 32 Pixeln Kante wird nur genommen, wenn nichts Besseres zu
haben ist. Höchstens fünf Abrufe je Webseite (die Seite zählt mit, gelesen werden höchstens
512 kB davon) innerhalb von acht Sekunden, jeder davon — die Seite, jedes Symbol, jede
Weiterleitung — über den abgesicherten Client; eine Seite kann den Server also nicht ins
lokale Netz schicken. Sender, die dieselbe Webseite nennen, teilen sich einen Blick darauf.
Die erste Anfrage nach einem solchen Logo kann deshalb einige Sekunden dauern; Logos erst beim
Anzeigen laden und nie eine Liste darauf warten lassen.

Die Route antwortet mit
`Cache-Control: private, max-age=86400` und einem `ETag` und behält, was sie geholt hat — und
was sich nicht holen ließ —, eine Weile im Arbeitsspeicher; erneutes Fragen ist also billig.
Angenommen werden JPEG, PNG, WebP, GIF und Windows-Icon-Dateien (`.ico`, ausgeliefert und
gespeichert als PNG ihres größten Bildes), erkannt am Inhalt; SVG nicht (es kann Skripte und
Verweise auf fremde Server enthalten, und der Server rendert es nicht). Mit `size` kommt ein
größeres Logo als JPEG, ein durchsichtiges als PNG.

Ein Sender, der hinzugefügt wird, behält das so gefundene Logo — das des Verzeichnisses, sonst
das Symbol seiner Webseite; ein per Adresse hinzugefügter Sender bekommt das Symbol der
mitgegebenen `homepage`. Ein gespeicherter Sender, der noch kein Logo hat (aus dem Verzeichnis
oder mit einer Webseite), bekommt es im Hintergrund, sobald die Senderliste gelesen wird — die
Liste selbst wartet nicht darauf, `has_logo` ist bei einem späteren Abruf wahr. Am
gespeicherten Sender behält `has_logo` seine strenge Bedeutung: Das Logo ist da.

**Datenschutz.** Das Verzeichnis ist [radio-browser.info](https://www.radio-browser.info/), ein
freier Gemeinschaftskatalog. Der Server schickt ihm die Suchwörter (und einen Ländercode), wenn
jemand sucht, und die id eines Senders, wenn er hinzugefügt wird oder wenn sein Logo gefragt
ist und der Server die Auskunft des Verzeichnisses nicht mehr weiß; nichts über Nutzer oder die
Sammlung. **Der Server ruft außerdem die Webseite des Senders ab** — die Homepage, die das
Verzeichnis nennt, oder die beim Hinzufügen per Adresse angegebene — und die Symbole, die diese
Seite nennt, wann immer ein Logo gefragt ist und das Verzeichnis keines hat, das sich laden
lässt: wenn das Logo eines Suchtreffers angefragt wird, wenn ein Sender hinzugefügt wird, und
im Hintergrund für gespeicherte Sender ohne Logo (Letzteres auch bei
abgeschaltetem Verzeichnis). Diese Anfragen tragen nichts außer der Kennung
`Autoreverse/<Version>`. Sender, ihre Webseiten und die Server der Logos sehen die Adresse des
Servers, nicht die des Hörers.

### Fernsteuerung

Die Geräte eines Kontos sehen, was auf den anderen läuft, steuern einander und übernehmen die
Wiedergabe voneinander. `GET /server/info` nennt `devices: true`, wo ein Server das anbietet; bei
unter Einstellungen → Server abgeschalteter Fernsteuerung antwortet jede Route unten mit `404` „Die Fernsteuerung ist auf diesem
Server abgeschaltet".

**Der Server ist nur Vermittler.** Er hält die Geräte im Arbeitsspeicher, je Konto getrennt, und
nichts davon in der Datenbank: Nach einem Neustart kennt er kein Gerät, und die Geräte verbinden
und melden sich von selbst neu. Server → Gerät ist ein langlebiges `GET` mit Server-Sent Events;
Gerät → Server sind gewöhnliche Anfragen. Authentifizierung wie überall (Bearer-Token oder
Sitzungs-Cookie).

Ein Gerät erzeugt sich einmal eine `device_id` und behält sie: 8–64 Zeichen aus `[A-Za-z0-9-]`
(eine UUID). Kennungen gelten je Konto – dieselbe Kennung in zwei Konten sind zwei Geräte.

| Anfrage                                   | Antwort                                                                |
| ----------------------------------------- | ---------------------------------------------------------------------- |
| `GET /me/devices/events?device_id=&name=&kind=` | `200 text/event-stream`, siehe unten. `name` 1–60 Zeichen, `kind` eines von `desktop`, `phone`, `tablet`, `web`, `car`; sonst `422`. |
| `GET /me/devices`                         | `200 {"devices": [Device…]}` – dieselbe Liste, für einen Client ohne Strom. |
| `PUT /me/devices/{device_id}/state`       | Body `State`. `204`; `422` bei falscher Form oder überschrittenen Grenzen; `409` „Das Gerät ist nicht verbunden" für ein Gerät, das seinen Strom noch nicht geöffnet hat. |
| `POST /me/devices/{device_id}/commands`   | Body `{"command", "value"?, "from": device_id}`. `202`; `409`, wenn das Ziel unbekannt oder offline ist oder gerade keinen Strom hat; `422` bei falschem Befehl oder Wert; `429` jenseits von 20 Befehlen je Sekunde an ein Gerät. |
| `POST /me/devices/{device_id}/transfer`   | Body `{"from": device_id \| null, "play"?: bool}` (`play` ist ohne Angabe `true`). `202`; `409`, wenn es nichts zu übernehmen gibt oder das Ziel nicht verbunden ist. |
| `DELETE /me/devices/{device_id}`          | `204`. Das Gerät meldet sich ab (beim Abmelden): sofort weg, sein Strom beendet. |

**Der Strom.** Die Antwort trägt `Cache-Control: no-cache` und `X-Accel-Buffering: no`. Solange
sie steht, ist das Gerät online. Bricht sie ab, bleibt das Gerät für eine Karenz von 20 Sekunden
in der Liste; ein Wiederverbinden in dieser Zeit ist für die anderen unsichtbar. Eine zweite
Verbindung mit derselben `device_id` ersetzt die erste, die der Server beendet. Alle 15 Sekunden
kommt eine Kommentarzeile `: ping`. Ereignisse, jeweils eine `event:`-Zeile und eine
`data:`-Zeile mit JSON:

| Ereignis    | Daten                                                                               |
| ----------- | ----------------------------------------------------------------------------------- |
| `devices`   | `{"devices": [Device…]}` – sofort nach dem Verbinden und bei jeder Änderung: ein Gerät kommt oder geht, ein Name, das aktive Gerät, ein Zustand in etwas Wesentlichem (alles außer der Position, die mit der Uhr fortschreitet; ein Sprung zählt). |
| `command`   | `{"command": string, "value": Zahl \| string \| bool \| null, "from": device_id}` – für dieses Gerät. |
| `take_over` | `{"state": State, "from": device_id, "play": bool}` – dieses Gerät soll diesen Zustand spielen. |

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

Nur Online-Geräte stehen in der Liste (`online` ist immer `true`), nach Namen sortiert. `state`
ist `null`, bis das Gerät einen gemeldet hat. `active` kennzeichnet höchstens ein Gerät: das, das
zuletzt `playing: true` gemeldet oder zuletzt die Wiedergabe übernommen hat; es bleibt auch
pausiert aktiv, und geht es offline, ist keines aktiv.

**Zustand.** Ein Gerät meldet, was es spielt: sofort, wenn sich etwas ändert, sonst alle
10 Sekunden während der Wiedergabe. `position` und `duration` sind Sekunden im laufenden Titel
bei Musik, sonst im ganzen Buch oder in der ganzen Folge. `volume` ist 0–1, oder `null`, wo sie
sich nicht fernregeln lässt. `repeat` ist `off`, `all` oder `one`. `media.kind` ist `album`,
`book`, `episode`, `track` oder `station`. `queue` (höchstens 300) ist die Liste ab dem laufenden
Eintrag, der laufende zuerst: Ein Titel nennt `item_id` und `track_id`, ein ganzes Buch oder eine
Folge nur `item_id`. `context` ist eine Zeile zum Anzeigen (ein Albumname, „Titel-Radio: …").
`at` ist der Zeitpunkt der Messung auf dem Gerät, eine beliebige ISO-8601-Zeit mit Offset; der
Server reicht sie als UTC **mit Millisekunden** weiter (`…T12:00:00.000Z` – anders als die
übrigen Zeitstempel dieser API). Die Zuschauer rechnen die Position selbst fort: `position` plus
(jetzt − `at`), solange `playing`. Felder, die ein Gerät weglässt, kommen mit ihren
Standardwerten zurück (`media`, `volume`, `context`: `null`; `duration`: 0; `shuffle`: false;
`repeat`: `off`; `queue`: `[]`); `playing`, `position` und `at` sind Pflicht. Nach jedem
(Neu-)Verbinden des Stroms einmal melden: Ein neu gestarteter Server hat den Zustand vergessen,
und `PUT …/state` ist `409`, bis der Strom steht.

**Befehle.** `play`, `pause`, `toggle`, `next`, `previous`, `stop` haben keinen Wert; `seek`
nimmt Sekunden (≥ 0, gezählt wie `position`), `volume` 0–1, `shuffle` einen Wahrheitswert,
`repeat` `off`, `all` oder `one`. Ein Gerät übergeht, was es nicht kann. `from` ist die eigene
`device_id` des Absenders; der Absender braucht keinen Strom. Befehle erreichen nur Geräte des
eigenen Kontos – das Gerät eines anderen ist schlicht unbekannt (`409`).

**Übernahme.** `POST …/transfer` schickt dem Ziel `take_over` mit dem Zustand, den die Quelle
zuletzt gemeldet hat – `from`, oder bei `null` das aktive Gerät –, dessen `position` auf jetzt
fortgerechnet (nach der Uhr des Servers seit Eingang der Meldung; `at` ist dann die Zeit des
Servers), und schickt der Quelle den Befehl `stop`. Das Ziel baut daraus seine Wiedergabe – Buch
oder Folge: `POST /play/{item_id}` und an `position` springen; Musik: die `queue` als
Titelliste, Start bei `position` im ersten; Sender: `station_id` – und meldet dann selbst seinen
Zustand. Erst diese Meldung macht es zum aktiven Gerät (auch wenn es pausiert übernommen hat,
`play: false`).

**Grenzen.** Höchstens 20 Geräte je Konto; ein weiteres verdrängt das am längsten stille. Ein
Strom, der nicht gelesen wird, hat einen begrenzten Puffer; läuft er über, beendet der Server den
Strom, und das Gerät verbindet neu. Ein Abmelden beendet die Ströme seines Tokens sofort; ein
Token, das auf anderem Weg ungültig wird (Passwortwechsel, gelöschtes Konto), beendet seine
Ströme binnen 30 Sekunden.

**Der Web-Player** ist ein Gerät der Art `web` („Firefox auf Linux"), seine Kennung liegt in
`localStorage`. Alle Tabs eines Browsers sind dieses eine Gerät; genau einer von ihnen hält den
Strom (der, der spielt – abgestimmt über einen `BroadcastChannel`). Der Knopf „Geräte" im
Mini-Player und im Vollbild-Player zeigt die Geräte; ein Klick überträgt die Wiedergabe dorthin.
Läuft auf einem anderen Gerät etwas und im Browser nichts, ist der Mini-Player dessen
Fernbedienung: „Läuft auf …", Play/Pause, Zurück/Weiter, Position, Lautstärke und „Hier
weiterhören". Shuffle und Wiederholen kennt der Web-Player nicht und übergeht diese beiden
Befehle.

### Server-Einstellungen

Was ein Verwalter unter Einstellungen → Server einstellt, für Clients, die dieselbe Seite anbieten.
Beide Routen nur für Verwalter (sonst `403`); jede Änderung wird gespeichert und wirkt sofort, ohne
Neustart. Die Felder und ihre Grenzen stehen im englischen README unter
[Server settings](README.md#server-settings).

- `GET /server/settings` liefert alle Werte; vom API-Schlüssel nur `ai.has_api_key`.
- `PATCH /server/settings` nimmt beliebige dieser Felder, ändert genau diese und antwortet wie
  `GET`. In `ai` setzt `api_key` den Schlüssel, `null` oder `""` löscht ihn, ohne das Feld bleibt
  er. Eine Änderung wird als Ganzes geprüft und als Ganzes mit `422` und deutschem `detail`
  abgelehnt.

### Wiedergabelisten

Eine Wiedergabeliste gehört dem Konto, das sie angelegt hat. Ihr Besitzer kann sie mit **den
anderen Konten dieses Servers** teilen — öffentlich ist hier nie etwas — und ihnen auch die
Einträge überlassen. Alles Folgende kommt hinzu: Ein Client, der nichts davon kennt, arbeitet
weiter wie bisher, und `GET /playlists` ohne Parameter liefert weiterhin nur die eigenen.

**Das Playlist-Objekt** — in `GET /playlists`, `GET /playlists/{id}` (dort mit `entries`) und in
jeder Antwort, die eine Wiedergabeliste zurückgibt (`POST /playlists`, `PATCH`, `PUT …/cover`,
der Import):

```json
{
  "id": 7, "name": "Zugfahrt", "created_at": "2026-11-13T18:04:11Z", "entry_count": 42,
  "description": "Für lange Strecken", "folder": "Reisen", "has_cover": true,
  "owner": "oskar", "mine": true, "shared": true, "collaborative": false,
  "can_edit": true, "duration": 9874.5
}
```

| Feld            | Typ            |                                                                                     |
| --------------- | -------------- | ----------------------------------------------------------------------------------- |
| `description`   | string \| null |                                                                                     |
| `folder`        | string \| null | Name eines Ordners. Wiedergabelisten mit gleichem Namen bilden einen Ordner; es gibt kein Ordner-Objekt und keine Verschachtelung. |
| `has_cover`     | bool           | Es gibt ein hochgeladenes Cover. Ohne eines setzt der Client wie bisher ein Mosaik aus den Einträgen zusammen. |
| `owner`         | string         | Kontoname des Besitzers.                                                            |
| `mine`          | bool           | Das anfragende Konto ist der Besitzer.                                              |
| `shared`        | bool           | Für alle Konten dieses Servers sichtbar.                                            |
| `collaborative` | bool           | Andere Konten dürfen Einträge hinzufügen, entfernen und umsortieren. Wird so gemeldet, wie es gewählt ist, wirkt aber nur, solange `shared` gilt. |
| `can_edit`      | bool           | Das anfragende Konto darf die Einträge ändern: der Besitzer, oder jeder bei einer geteilten, gemeinsamen Liste. |
| `duration`      | number         | Sekunden, alle Einträge zusammen (ein Eintrag für ein ganzes Item zählt all dessen Titel; eine noch nicht geladene Folge das, was ihr Feed nennt). Immer vorhanden. |

**`GET /playlists?scope=mine|shared|all`** — `mine` (Vorgabe) sind die eigenen, `shared` die
geteilten *anderer* Konten, `all` beides: eigene zuerst, dann die der anderen, jeweils nach
Namen. Alles andere ist `422`.

**`PATCH /playlists/{id}`** nimmt neben `name` beliebige von `description` (≤ 2000 Zeichen; leer
oder `null` löscht), `folder` (≤ 100 Zeichen, ohne Leerraum an den Rändern; leer oder `null` =
kein Ordner), `shared` und `collaborative` (Wahrheitswerte). Ein fehlendes Feld bleibt, wie es
ist; ein Body mit keinem der fünf ist `422` wie bisher. `POST /playlists` nimmt dieselben vier
neben seinem `name`.

**Wer was darf**

| | Besitzer | anderes Konto, privat | …, geteilt | …, geteilt und gemeinsam |
| --- | --- | --- | --- | --- |
| lesen, abspielen, Cover, Export | ja | `404` | ja | ja |
| Einträge hinzufügen, entfernen, verschieben | ja | `404` | `403` | ja |
| Name, Beschreibung, Ordner, Freigabe, Cover, Löschen | ja | `404` | `403` | `403` |

Eine private Wiedergabeliste eines anderen ist auf jeder Route `404 "Unbekannte Playlist"`, genau
wie eine, die es nicht gibt — auch für Verwalter.

**Cover.** `PUT /playlists/{id}/cover` nimmt das Bild entweder als Multipart-Formular mit der
Datei im Feld `image` (wie `POST /artists/{name}/image`) oder als Rumpf der Anfrage mit seinem
`Content-Type`. JPEG, PNG, WebP oder GIF, höchstens 5 MB (`413` darüber, `422` bei anderem Typ
oder leerer Datei). Antwort: `200` mit der Wiedergabeliste. `DELETE` entfernt es (`204`).
`GET /playlists/{id}/cover?size=<Pixel>` liefert es wie jedes Cover (Vorschaugrößen, `ETag`) —
und ist `404 "Kein Cover"`, wenn die Liste kein eigenes hat: Der Server denkt sich keines aus.

**Export.** `GET /playlists/{id}/export?format=m3u8` (Vorgabe) antwortet mit
`Content-Type: audio/x-mpegurl; charset=utf-8` und `Content-Disposition: attachment;
filename="<Name>.m3u8"; filename*=UTF-8''<Name>.m3u8` — der Name ohne das, was ein Dateisystem
ablehnt, die erste Form in ASCII. Der Inhalt ist Extended M3U:

```
#EXTM3U
#PLAYLIST:Zugfahrt
#EXTINF:252,Björk - Human Behaviour
Björk/Debut/01 Human Behaviour.flac
```

Je Titel ein `#EXTINF:<Sekunden, gerundet>,<Interpret> - <Titel>` (nur der Titel, wo kein
Interpret bekannt ist) und darunter der Pfad der Datei **relativ zum Wurzelverzeichnis ihrer
Bibliothek** — absolute Pfade des Servers verlassen ihn nie; eine Datei unter keiner Wurzel
steht nur mit ihrem Dateinamen da. Ein Eintrag, der ein ganzes Item ist (Album, Buch), wird in
seine Titel aufgelöst. `?format=json` gibt dieselbe Liste für Werkzeuge: `{"name",
"description", "tracks": [{"title", "artist", "album", "duration", "path"}]}`.

**Import.** `POST /playlists/import` mit `{"name": "…", "content": "<M3U- oder M3U8-Text,
höchstens 2 MB>", "folder": "…"?}` antwortet mit `201`:

```json
{ "playlist": { … }, "matched": 40, "unmatched": ["The Knife - Heartbeats", "Mix/99 Fehlt.mp3"] }
```

Jede Zeile wird in dieser Reihenfolge zugeordnet:

1. **Über den Pfad**: die längste Folge aus Ordnern und Dateiname, die sie mit einem Titel der
   Sammlung teilt, vom Ende her gezählt — ein Pfad relativ zu einer Bibliothekswurzel passt
   also ebenso wie einer unter fremder Wurzel (`/Volumes/Musik/…`, `C:\Musik\…`), mit
   Backslashes, URL-Kodierung oder `file://`. Groß/Klein und Akzente spielen keine Rolle. Ein
   Treffer allein über den Dateinamen zählt nur, wenn er eindeutig ist und das `#EXTINF` der
   Zeile ihm nicht widerspricht.
2. **Über `#EXTINF`**: Interpret und Titel, verglichen ohne Groß/Klein, Akzente und
   Klammerzusätze („(Remastered 2011)"); bei mehreren Treffern gewinnt der, dessen Dauer
   höchstens ±3 s von der des `#EXTINF` abweicht.

Die Reihenfolge der Datei bleibt, Doppelte bleiben. Nicht Gefundenes kommt als lesbare Zeilen
in `unmatched` zurück (der `#EXTINF`-Text, sonst der Pfad). Wurde **nichts** gefunden, entsteht
keine Wiedergabeliste: `422` mit `{"detail": "…", "unmatched": […]}`. Text über 2 MB ist `413`.

### Als Link teilen

`GET /server/info` nennt `web_url`: die öffentliche Adresse der Weboberfläche (`ORIGIN`), oder
`null`, wenn der Betreiber keine gesetzt hat. Clients bauen daraus Links auf deren Seiten. **Wer
so einen Link öffnet, braucht ein Konto auf diesem Server**; öffentliche Links ohne Anmeldung
gibt es nicht. Die Pfade sind stabil:

| Was             | Pfad                                           |
| --------------- | ---------------------------------------------- |
| Album           | `/library/albums/{id}`                         |
| Einzelner Titel | `/library/albums/{Album-id}?track={Titel-id}` — die Albumseite, die Zeile hervorgehoben und ins Bild geholt |
| Hörbuch         | `/library/books/{id}`                          |
| Podcast         | `/library/podcasts/{id}`                       |
| Interpret       | `/library/albums?artist={Name, URL-kodiert}` — die Alben des Interpreten |
| Wiedergabeliste | `/library/playlists/{id}`                      |

Die Weboberfläche hat auf jeder dieser Seiten „Link kopieren"; kopiert wird die Adresse unter
dem Ursprung, den der Browser selbst benutzt.

### Entdecken-Feed

`GET /ai/feed` ist alles, was die Seite „Entdecken" dem fragenden Benutzer zeigt. Trotz des Pfads
funktioniert die Route ohne Sprachmodell: Nur `suggestions`, `mixes`, `artists` und `wanted`
stammen von einem (und bleiben ohne leer). Jede Liste ist immer vorhanden, leer, wenn es nichts
zu zeigen gibt.

| Feld               | Typ                 | Quelle     | Inhalt                                                                  |
| ------------------ | ------------------- | ---------- | ----------------------------------------------------------------------- |
| `enabled`          | Wahrheitswert       | —          | Ob ein Modell eingerichtet ist.                                         |
| `refreshing`       | Wahrheitswert       | —          | Ein Lauf für die KI-Listen ist im Gang; in ein paar Sekunden erneut fragen. |
| `has_history`      | Wahrheitswert       | —          | Der Benutzer hat Hörzeit oder Favoriten.                                |
| `checking_catalog` | Wahrheitswert       | —          | Der Abgleich mit MusicBrainz läuft noch; `wanted` kann wachsen.         |
| `generated_at`     | Zeichenkette / null | —          | Wann die KI-Listen erzeugt wurden.                                      |
| `truncated`        | Wahrheitswert       | —          | Die Sammlung war zu groß, um sie dem Modell ganz zu zeigen.             |
| `suggestions`      | Albumvorschlag[]    | Modell     | Alben der Mediathek zum Weiterhören, je mit `reason`.                   |
| `mixes`            | Mix[]               | Modell     | `{name, description, cover_ids, tracks}`; `tracks` wie bei `/ai/playlist`. |
| `artists`          | Interpret[]         | Modell     | `{name, reason, albums}`: Interpreten, die der Mediathek fehlen.        |
| `wanted`           | Wunschalbum[]       | Modell     | `{artist, title, year, reason}`: Alben, die der Mediathek fehlen.       |
| `rediscover`       | Albumvorschlag[]    | Datenbank  | Früher gehörte Alben, seit 90 Tagen nicht mehr, die meistgehörten zuerst. |
| `fresh`            | Albumvorschlag[]    | Datenbank  | In den letzten 60 Tagen hinzugekommen, ungehört, von einem gehörten Interpreten oder Genre. |
| `new_episodes`     | Folge[]             | Datenbank  | Die neuesten ungehörten Folgen der abonnierten Podcasts, höchstens 12.  |
| `unstarted_books`  | Buch[]              | Datenbank  | Hörbücher, die der Benutzer nicht begonnen hat, zuletzt hinzugefügte zuerst, höchstens 12. |
| `continue_series`  | Buch[]              | Datenbank  | Je Serie, in der der Benutzer ein Buch beendet hat, der nächste nicht begonnene Band, höchstens 12. |
| `rediscover_books` | Buch[]              | Datenbank  | Begonnene, nicht beendete Hörbücher, seit 30 Tagen nicht angerührt, höchstens 12. |

Ein Albumvorschlag ist `{id, title, artist, year, has_cover, reason}`; `reason` ist in den Listen
aus der Datenbank leer.

Die Listen mit Quelle „Datenbank" werden bei jeder Anfrage berechnet und nicht mit der Antwort
des Modells gespeichert: Eine eben erschienene Folge oder ein eben begonnenes Buch steht schon in
der nächsten Antwort. `new_episodes`, `unstarted_books`, `continue_series` und `rediscover_books`
kamen später hinzu; gegenüber einem älteren Server muss ein Client ein fehlendes Feld wie eine
leere Liste behandeln.

Eine **Folge** ist dasselbe Objekt wie in `GET /podcasts/new`, und `new_episodes` ist der Anfang
jener Liste (nicht beendete Folgen, die seit dem Abonnieren erschienen sind, neueste zuerst):

| Feld            | Typ                 | Bedeutung                                                                |
| --------------- | ------------------- | ------------------------------------------------------------------------ |
| `id`            | Ganzzahl            | Item-ID der Folge — für `/play/{id}`, `/items/{id}`, `/episodes/{id}/download`. |
| `title`         | Zeichenkette        |                                                                          |
| `published_at`  | Zeichenkette / null | ISO-8601, UTC.                                                           |
| `podcast_id`    | Ganzzahl            | Item-ID des Podcasts (das Eltern-Item der Folge).                        |
| `podcast_title` | Zeichenkette        |                                                                          |
| `has_cover`     | Wahrheitswert       | `GET /items/{id}/cover` liefert ein Bild — das der Folge oder das ihres Podcasts. |
| `downloaded`    | Wahrheitswert       | Ob die Datei auf dem Server liegt. Wenn nicht, erst `POST /episodes/{id}/download`; `/play/{id}` antwortet bis dahin mit `409`. |
| `duration`      | Zahl                | Sekunden: gemessen, sobald geladen, sonst die Angabe des Feeds, sonst `0`. |
| `position`      | Zahl                | Gespeicherte Position des Benutzers in Sekunden, `0`, wenn nicht begonnen. |

Ein **Buch** ist die Item-Zusammenfassung von `GET /items` mit drei weiteren Feldern; ein Client
liest es mit dem Modell, das er schon hat:

| Feld            | Typ                 | Bedeutung                                                              |
| --------------- | ------------------- | ---------------------------------------------------------------------- |
| `id`            | Ganzzahl            | Item-ID — für `/play/{id}`, `/items/{id}`, `/items/{id}/cover`.        |
| `kind`          | Zeichenkette        | Hier immer `"book"`.                                                   |
| `title`         | Zeichenkette        |                                                                        |
| `has_cover`     | Wahrheitswert       |                                                                        |
| `author`        | Zeichenkette / null |                                                                        |
| `artist`        | Zeichenkette / null | Bei Büchern `null`.                                                    |
| `series`        | Zeichenkette / null | Name der Serie.                                                        |
| `year`          | Ganzzahl / null     |                                                                        |
| `missing_since` | Zeichenkette / null | Hier immer `null`: Fehlendes wird ausgelassen.                         |
| `published_at`  | Zeichenkette / null | Bei Büchern `null`.                                                    |
| `narrator`      | Zeichenkette / null | Zusätzlich zur Zusammenfassung.                                        |
| `series_index`  | Zahl / null         | Die Bandnummer in `series` (kann gebrochen sein, z. B. `2.5`). Teil jeder Item-Zusammenfassung. |
| `duration`      | Zahl                | Zusätzlich: Gesamtlänge in Sekunden, `0`, wenn unbekannt.              |
| `position`      | Zahl                | Zusätzlich: gespeicherte Position in Sekunden; `0` außer in `rediscover_books`. |

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

Wie die Buchlisten entstehen: Ein Buch gilt als begonnen, sobald der Benutzer darin eine Position
größer als 0 hat oder es beendet hat. Eine Serie wird an Name **und** Autor erkannt. Ihre Bände
sind nach `series_index` geordnet, dann nach einer Nummer im Titel, dann nach dem Titel; „der
nächste" ist der erste nicht begonnene Band nach dem am weitesten hinten beendeten — nach Band 3
wird Band 4 angeboten, auch wenn Band 1 übersprungen wurde, und nichts mehr, wenn der letzte Band
beendet oder begonnen ist. Die Serie, in der zuletzt etwas beendet wurde, steht vorn. Ein Buch
kann in `unstarted_books` und in `continue_series` zugleich stehen.

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
einem eigenen Build-Rechner (`e2e/run-in-vm.sh`), nicht auf dem Entwicklerrechner; wer sie
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
  library/      Abfragen, Fortschritt, Lesezeichen, Favoriten, Wiedergabelisten, Titel-Radio, Metadaten,
                Songtexte, Meistgehörtes und Jahresrückblick
  podcasts/     Feeds, Verzeichnissuche, Episoden-Downloads, Vorhalten, Hintergrundlauf
  stations/     Internetradio: Verzeichnis, Prüfen einer Adresse, Strom-Proxy, ICY-Titel
  devices/      Fernsteuerung: die Online-Geräte je Konto, ihre Ereignisströme
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
