# Plan: Offene Arbeiten über alle vier Projekte

Stand 2026-11-13. Dieser Plan fasst zusammen, was nach der großen Runde vom 11. bis 13. November
(Fehlersuche, neue Navigation, Abgleich mit Spotify) noch offen ist. Er liegt im Server-Repo, weil
der Server die Schnittstelle vorgibt, an der sich die drei Apps ausrichten.

Die vier Projekte:

| Projekt | Repo | Technik |
| --- | --- | --- |
| Server | `mojm-development/autoreverse-server` (GitHub) | Rust: axum, sqlx, Postgres, Web-Oberfläche |
| Desktop | `autoreverse/autoreverse-omarchy` (GitLab) | Qt 6 / QML, C++ |
| Android | `autoreverse/autoreverse-android` (GitLab) | Kotlin, Compose, Media3 |
| iOS | `autoreverse/autoreverse-ios` (GitLab) | Swift 6, SwiftUI, CarPlay |

## 1. Zuerst: offene Anfragen mergen und ausrollen

Beim Schreiben dieses Plans waren diese Anfragen offen. Der Server gehört jeweils zuerst
ausgerollt; die Apps blenden neue Funktionen gegen einen älteren Server still aus.

| Projekt | Anfrage | Inhalt |
| --- | --- | --- |
| Server | PR 31 | Mixe wechseln die Interpreten ab |
| Desktop | MR 13 | Senderlogos in der Sendersuche |
| Android | MR 14 | Senderlogos in der Sendersuche |
| iOS | MR 10 | Interpreten als normaler Bereich des Musik-Tabs; Senderlogos |

Nach dem Ausrollen von PR 31 auf „Entdecken" einmal „Neu mischen" drücken: Die bestehenden Mixe
liegen im gespeicherten Feed und behalten sonst ihre alte Reihenfolge.

Was beim Ausrollen der letzten Server-Stände einmalig passiert:

- **Erster Scan dauert länger.** Er liest die Tags aller Titel, um Songtexte zu erkennen.
- **Lautheit wird im Hintergrund gemessen** (`AUTOREVERSE_LOUDNESS`, `AUTOREVERSE_LOUDNESS_THREADS`).
  Grob 40 Minuten für 20.000 Titel mit vier Threads auf einem schnellen Rechner, auf einem NAS
  deutlich länger. Der Durchgang überlebt Neustarts.
- **`ORIGIN` muss gesetzt sein**, sonst blenden die Apps „Teilen" aus.
- **Reverse-Proxy:** Der Ereignisstrom der Fernsteuerung (`/me/devices/events`) darf nicht
  gepuffert werden. Der Server schickt `X-Accel-Buffering: no`; bei anderen Proxys als nginx prüfen.
- **KI-Modell:** `AUTOREVERSE_AI_MODEL="claude-sonnet-5-5"` stellt von Haiku auf Sonnet um.

## 2. Am Gerät prüfen, bevor weitergebaut wird

Alles aus dieser Runde ist durch Unit-Tests und Builds abgesichert. Auf einem Telefon, im Auto, im
Browser oder gegen den Live-Server ist fast nichts gelaufen. Echt ausprobiert wurden nur drei
Dinge, jeweils auf einem Wegwerf-Server: die Radiosender (Suche, Hinzufügen, Strom, laufender
Titel), die Fernsteuerung mit zwei simulierten Geräten und die Senderlogos.

Die Reihenfolge unten geht nach Risiko.

### iOS

- **Migration der lokalen Datenbank** (v15 bis v21) mit echten Downloads. Die Download-Ordner
  ziehen dabei in einen Ordner je Konto um (`Downloads/konto-<hash>/`). Das ist der heikelste Teil.
- **Podcasts abspielen:** Eine Folge, die der Server noch nicht geholt hat, startet nach dem Holen.
- **CarPlay:** gleicht selbst ab, zeigt Fehler, neue Darstellung mit Cover-Regalen und Kopfbereich
  (ab iOS 26.0 bzw. 26.4, darunter die alte Liste). Ungeklärt: ob die Kopf-Knöpfe im Musik-Tab
  erscheinen, ob der Fortschritt auf den Karten sichtbar ist, ob die Liste beim Nachladen der Cover
  springt.
- **Sitzungen schließen** nach fünf Minuten Pause und im Hintergrund: reicht die Hintergrundzeit?
- **Lautstärke angleichen:** fällt der Pegelwechsel zwischen verschieden lauten Titeln als Sprung auf?
- **Radiosender:** spielt der Strom mit der Anmeldung, gibt Pause den Server-Strom frei, Live-Anzeige
  auf dem Sperrbildschirm.
- **Fernsteuerung:** Abriss und Wiederaufbau der Verbindung, Übernahme mit echtem Ton.
- **Die UI-Tests** (`UITests/`) sind an die neue Navigation angepasst, aber nie gelaufen.

### Android

- **Offline-Downloads:** der ganze Ablauf mit Benachrichtigung, Flugmodus, Fortsetzen nach Neustart,
  Nachsenden des Fortschritts.
- **Warteschlange:** Ziehen und Wischen im Blatt, Mischen mit eingereihten Titeln.
- **Benachrichtigung und Android Auto:** richtige Knöpfe je Medienart, Paging, Fehleranzeige.
- **Radiosender:** spielen MP3-, AAC- und Ogg-Ströme, gibt Pause die Verbindung frei.
- **Fernsteuerung:** hält der Strom im Doze, wann beendet Android den pausierten Dienst.

### Desktop

- **Scrollen** mit echter Maus und echtem Touchpad unter Wayland. 120 px je Raste steht in
  `src/main.cpp` und ist eine Zahl.
- **Lückenlos:** echte Musikdateien (MP3, FLAC, Opus). Gemessen sind 113 ms → 11 ms nur mit stillen
  Testdateien.
- **MPRIS** über D-Bus ist ganz ungetestet (`mpris.cpp` ist nicht im Test-Build).
- **Datei-Dialoge und Zwischenablage** unter Wayland (Playlist-Import, -Export, „Link kopieren").

### Web

Keine der neuen Seiten lief in einem Browser: Radio, Statistik, Rückblick, Songtext im Player,
Geräte-Panel, Playlist-Ordner, mobile Tab-Leiste mit „Entdecken". Die Playwright-Tests
(`e2e/run-in-vm.sh`) liefen in der ganzen Runde nicht; einige Specs sind angepasst, eine zur
Playlist-Übersicht kann anschlagen, wenn im Seed eine geteilte Playlist eines anderen Kontos liegt.

## 3. Bekannte Fehler und Schwächen

Nach Projekt, das Wichtigste zuerst.

### Server

- **Album-Lautheit bleibt stehen**, wenn ein Titel aus einem Album entfernt wird, bis sich eine
  Datei des Albums ändert (`src/scanner/loudness/`).
- **Cover-Dateien von Playlists** eines gelöschten Kontos bleiben auf der Platte.
- **Playlist-Import** liest alle Titel der Sammlung in den Speicher. Mit sechsstelliger Titelzahl
  ungetestet (`src/library/m3u.rs`).
- **`books_finished` im Rückblick** nimmt den Zeitpunkt der letzten Fortschrittsänderung als
  Näherung für das Beendet-Datum.
- **Durchsichtige Logos gespeicherter Sender** über 256 px bekommen beim Verkleinern einen schwarzen
  Hintergrund (die Route liefert JPEG).
- **Alte SHOUTcast-v1-Sender**, die mit `ICY 200 OK` antworten, scheitern vermutlich am HTTP-Client.
- **Fehlertexte sind deutsch**, auch für die englische Oberfläche der Apps.

### Desktop

- **Übernahme von Musik** kommt als Liste einzelner Titel an: Ein übernommenes Album verliert
  Album-Fortschritt und Album-Lautheit (`player.cpp`, `adoptTracks`).
- **„Now playing"** wird nicht zur Fernbedienung, nur die Player-Leiste.
- **Doppelbuchung:** Geht die Antwort auf das Schließen einer Sitzung verloren, zählt diese eine
  Sitzung doppelt (`src/listenqueue.*`).
- **Über Albumgrenzen** bleibt beim Titelwechsel eine Lücke von der Dauer einer Serverantwort.
- **Cover-Cache-Test** `aCachedCoverCostsNoRequestUntilItIsStale` fiel vor der Korrektur in 8 von
  150 Läufen durch. Die Ursache in `streamproxy.cpp` ist behoben, die Quote aber nicht neu gemessen.
- Genre- und Interpretenseiten laden höchstens 500 Alben; der Bild-Cache auf der Platte hat keine
  Obergrenze; die Lautstärke-Skala ist linear.

### Android

- **Automatisches Laden neuer Folgen** fehlt.
- **Downloads hängen an der Anmeldung:** Läuft sie ab (401), sind sie weg. Sauber wäre die Bindung
  an Server und Konto wie auf iOS.
- **Nach einem Absturz** laufen Downloads erst beim nächsten Öffnen der App weiter.
- **Buch mit eingereihter Musik:** Bei einer Übernahme geht die Musik dahinter verloren.
- **Anruf während Radio:** Der Strom bleibt offen.
- **Player-Logo eines Senders** bleibt Platzhalter, wenn der eine Abruf beim Start scheitert.
- Android Auto hat keine Playlists, Favoriten und Interpreten; die Senderliste hat keinen
  Offline-Stand; Hörbücher und Folgen lassen sich nicht einreihen.

### iOS

- **Anmeldename** wird nicht normalisiert: „Anna" und „anna" wären zwei Besitzer der Downloads.
- **„Teilen" fehlt** in der Suche, auf „Start", auf „Entdecken" und im Player.
- **Warteschlange nach Radio** übersteht keinen Neustart der App.
- **Kein Textknopf bei KI-Mixen** und „Playlist aus einem Satz": Der Server liefert dort kein
  `has_lyrics` (das wäre am Server zu ergänzen, `src/ai/music.rs`, `PlaylistTrack`).
- **Lücken beim Titelwechsel** bleiben beim Übergang ins Autoplay, beim Umlauf von „Alle
  wiederholen" und beim manuellen Springen.
- CarPlay schneidet lange Listen ab und hat nur für Alben ein Buchstabenregister.

### Alle Apps

- **Lautstärke angleichen senkt nur ab**, hebt nie an. Die Player aller drei Plattformen enden bei
  voller Lautstärke.
- **Titelgenaue Auswertungen** (Meistgehört, On Repeat, Rückblick) gibt es erst für das, was seit
  dem neuen Server gehört wurde.
- **Telefone sind in der Geräteliste nur sichtbar**, solange die App offen ist oder etwas spielt.
  Abhilfe wären Push-Nachrichten über Apple und Google; das ist bewusst nicht gebaut.

## 4. Noch nicht gebaut

In der Reihenfolge, in der ich es angehen würde.

1. **Crossfade (Überblenden).** Als Einziges von der Spotify-Liste offen. Zurückgestellt, bis die
   Übergänge aus der Lautstärke-Runde am Gerät gehört sind: Es greift am tiefsten in die Wiedergabe
   ein und lässt sich ohne Ohren nicht absichern. Desktop hat mit den zwei Abspielwerken
   (`Player`, zweites Deck) die Grundlage schon; Android und iOS brauchen je einen zweiten Player.
2. **Lückenlos im Web-Player.** Ein zweites `<audio>`-Element greift in Analyser, Media Session und
   Hörzeit-Zählung ein (`static/js/player/`).
3. **Songtexte aus einer externen Quelle.** Heute nur aus `.lrc`/`.txt` neben dem Titel oder aus den
   Tags. Eine externe Quelle hieße, Titel und Interpret an einen fremden Dienst zu schicken.
   **Entscheidung offen.**
4. **Öffentliche Links ohne Anmeldung.** Heute braucht der Empfänger eines geteilten Links ein
   Konto. Öffentliche Links bräuchten Ablaufdatum und Widerruf. **Entscheidung offen.**
5. **Streaming-Qualität wählen** (Umwandeln auf dem Server für Mobilfunk). Großer Brocken; nur
   sinnvoll, wenn wirklich mit knappem Datenvolumen gehört wird. **Entscheidung offen.**
6. **HLS-Radiosender** (`.m3u8`). Werden abgelehnt, weil der Server sonst beliebige Segment-Adressen
   auf Zuruf abrufen müsste.
7. **Equalizer**, **Chromecast**, **Widgets** (iOS, Android), **Siri- und Sprachbefehle für Musik
   und Podcasts**, **Playlist-Verwaltung in Android Auto**.
8. **Verwaltung in den Apps** (Nutzer, Metadaten, Duplikate). Gibt es nur im Web; in den Apps
   genügt vermutlich ein Verweis dorthin.
9. **Podcasts:** Folgen als gehört markieren, Tempo je Sendung, Stille kürzen.

Bewusst nicht vorgesehen: Deezer in jeder Form, Apple Music Radio (kopiergeschützt), ein eigener
Musikkatalog, Konzerte und Musikvideos.

## 5. Arbeitsumgebung

### Server

```
docker compose up -d postgres        # Datenbank für Tests und lokale Läufe (Port 5434)
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Stand zuletzt: 359 Unit-Tests, 656 Integrationstests, 119 JS-Tests.

### Desktop

```
bin/build
bin/test        # zuletzt 104 Tests
```

`moc` behandelt `//` in Raw-Strings als Kommentar, und innerhalb der Testklasse müssen geschweifte
Klammern in Raw-Strings insgesamt aufgehen.

### Android

```
export JAVA_HOME=/opt/android-studio/jbr ANDROID_HOME=$HOME/Android/Sdk PATH=/opt/android-studio/jbr/bin:$PATH
./gradlew --console=plain :core:domain:allTests :core:data:allTests :app:testDebugUnitTest :app:assembleDebug :app:lintDebug
```

### iOS

Gebaut und getestet wird auf dem Build-Mac (`ssh build-mac`).
Das Skript unten kopiert den Arbeitsbaum dorthin und läuft `swift test` in den vier Paketen sowie
einen Simulator-Build. Stand zuletzt: AutoreverseKit 121, Domain 414, Data 283, LibraryCache 136 + 18.

```bash
#!/bin/bash
# Aufruf: ios-check.sh packages|app|all
set -uo pipefail
SSH="ssh -o ControlMaster=auto -o ControlPath=$HOME/.ssh/cm/%r@%h-%p -o ControlPersist=4h -o ConnectTimeout=10"
SRC=$HOME/Projects/autoreverse/autoreverse-ios/
DST=builds/autoreverse-ios
WHAT="${1:-all}"
rsync -a --delete -e "$SSH" --exclude .git --exclude .build --exclude build --exclude DerivedData "$SRC" "build-mac:$DST/" || exit 9
$SSH build-mac "WHAT=$WHAT DST=$DST zsh -s" <<'REMOTE'
export PATH=/opt/homebrew/bin:$PATH
cd ~/$DST
if [[ $WHAT == packages || $WHAT == all ]]; then
  for p in AutoreverseKit Domain Data LibraryCache; do
    echo "=== swift test $p"
    (cd Packages/$p && swift test 2>&1 | grep -E "error:|✘|Test run with|Executed [0-9]+ tests" | sort -u | tail -25)
  done
fi
if [[ $WHAT == app || $WHAT == all ]]; then
  echo "=== xcodegen + xcodebuild"
  xcodegen 2>&1 | tail -2
  xcodebuild -project Autoreverse.xcodeproj -scheme Autoreverse \
    -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath ~/builds/autoreverse-ios-dd CODE_SIGNING_ALLOWED=NO build 2>&1 \
    | grep -E "error:|\*\* BUILD" | sort -u | tail -80
fi
REMOTE
```

Dazu zwei Dinge:

- **`Autoreverse.xcodeproj` ist eingecheckt** und wird aus `project.yml` erzeugt. Nach neuen Dateien
  unter `App/` muss das auf dem Mac erzeugte `project.pbxproj` zurück ins Repo.
- **Kein Test ohne Zeitlimit** (`.timeLimit(.minutes(1))`). Ein hängender Test hält die Sperre des
  Build-Verzeichnisses dauerhaft.

### Commits

- Commits sind signiert. Ist der Signierschlüssel nicht verfügbar, scheitert der Commit
  mit „failed to write commit object" und der Push mit „Could not read from remote repository".
- Keine Co-Authored-By-Zeilen von Werkzeugen in Commits und Anfragen.
- Je Runde ein Branch pro Repo; der Server zuerst, die Apps bauen gegen seine Schnittstelle.

## 6. Schnittstellen, die in dieser Runde dazukamen

Zum Nachschlagen; die genaue Beschreibung steht im README.

| Thema | Routen |
| --- | --- |
| Interpreten-Übersicht | `GET /artists/{name}/info`, `/artists/{name}/pictures/{index}` |
| Hörzeit | `POST /sessions/{id}/close` mit `listened`, `tracks`, `position: null`; `POST /me/listens` |
| Entdecken | `GET /ai/feed` mit `new_episodes`, `unstarted_books`, `continue_series`, `rediscover_books` |
| Titel-Radio | `GET /radio?track_id=|item_id=|artist=` |
| Verlauf und Auswertung | `GET /me/history`, `/me/top`, `/me/on-repeat`, `/me/repeat-rewind`, `/me/review` |
| Radiosender | `/radio/stations` (Liste, Suche, Strom, laufender Titel, Logo), `/radio/stations/search/logo` |
| Songtexte | `GET /tracks/{id}/lyrics`, `has_lyrics` an jedem Titel |
| Lautheit | `gain_db`, `album_gain_db`, `peak` an jedem Titel |
| Playlists | `scope`, Ordner, Beschreibung, Cover, `shared`, `collaborative`, Export, Import |
| Teilen | `web_url` in `/server/info` |
| Fernsteuerung | `/me/devices` (Ereignisstrom, Zustand, Befehle, Übernahme) |
