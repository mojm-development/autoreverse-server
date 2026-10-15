# Entwicklungsumgebung für Autoreverse Server (Rust).
#
# `make setup` einmal, danach `make dev`. Jedes Ziel, das die Datenbank braucht,
# startet sie selbst — es gibt keine Reihenfolge, die man kennen müsste.
#
# Der Server liest .env selbst (dotenvy). `cargo test` braucht keine .env: die
# Datenbank-URL für die Tests steht in .cargo/config.toml (Postgres auf Port 5434).

# macOS liefert GNU Make 3.81 aus, das kennt .ONESHELL noch nicht — jede Zeile eines
# Rezepts läuft in ihrer eigenen Shell. Mehrzeilige Logik steht deshalb als eine
# fortgesetzte Zeile da, statt sich stillschweigend auseinanderreißen zu lassen.
SHELL := /bin/bash
.DEFAULT_GOAL := help

COMPOSE := docker compose
DB_SERVICE := postgres
DB_USER := autoreverse

.PHONY: help setup dev run stop doctor \
        db db-stop db-reset db-shell db-logs password \
        check lint format test test-unit test-integration test-js test-e2e \
        build image stack stack-stop clean

help: ## Diese Übersicht
	@echo "Autoreverse Server — Entwicklungsumgebung"
	@echo
	@grep -hE '^[a-z][a-zA-Z0-9_-]*:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'
	@echo
	@echo "  Erstes Mal:  make setup && make dev"

# --- Einrichtung ------------------------------------------------------------

.env:
	@cp .env.example .env
	@echo "→ .env aus .env.example angelegt. Passwörter darin ändern, bevor es ernst wird."

dev-library:
	@mkdir -p dev-library/books dev-library/music dev-library/podcasts
	@echo "→ dev-library/{books,music,podcasts} angelegt."
	@echo "  Die Pfade trägst du nach dem ersten Login unter Einstellungen → Bibliotheken ein."

setup: .env dev-library ## Einmalig: .env, dev-library/, Datenbank, erster Build
	@$(MAKE) --no-print-directory db
	@cargo build
	@echo
	@echo "Fertig. Weiter mit: make dev"

doctor: ## Prüft, ob alles da ist, was die Umgebung braucht
	@ok=0; \
	for tool in cargo docker ffmpeg ffprobe node; do \
		if command -v $$tool >/dev/null; then echo "  ✓ $$tool"; \
		else echo "  ✗ $$tool fehlt"; ok=1; fi; \
	done; \
	if [ -f .env ]; then echo "  ✓ .env"; else echo "  ✗ .env fehlt (make setup)"; ok=1; fi; \
	if $(COMPOSE) ps --status running --services 2>/dev/null | grep -qx $(DB_SERVICE); then \
		echo "  ✓ Postgres läuft (Port 5434)"; \
	else echo "  ✗ Postgres läuft nicht (make db)"; ok=1; fi; \
	exit $$ok

# --- Datenbank --------------------------------------------------------------

db: .env ## Startet Postgres im Hintergrund und wartet, bis sie antwortet
	@$(COMPOSE) up -d $(DB_SERVICE)
	@printf "→ warte auf Postgres"; \
	for i in $$(seq 1 60); do \
		if $(COMPOSE) exec -T $(DB_SERVICE) pg_isready -U $(DB_USER) >/dev/null 2>&1; then \
			echo " — bereit."; exit 0; \
		fi; \
		printf "."; sleep 1; \
	done; \
	echo; echo "Postgres antwortet nicht. Log: make db-logs" >&2; exit 1

db-stop: ## Hält Postgres an (Daten bleiben erhalten)
	@$(COMPOSE) stop $(DB_SERVICE)

# Das Schema legt der Server beim nächsten Start selbst an (eingebettete Migrationen).
db-reset: ## Wirft die Datenbank samt Inhalt weg und legt sie leer neu an
	@read -p "Alle lokalen Daten löschen? [j/N] " answer; \
	if [ "$$answer" != "j" ]; then echo "Abgebrochen."; exit 0; fi; \
	$(COMPOSE) down -v && $(MAKE) --no-print-directory db

# Der erste Verwalter entsteht nur bei leerer Nutzertabelle aus .env — eine Datenbank mit
# vergessenem Passwort ließe sich sonst nur per SQL retten. Das Passwort wird abgefragt,
# nicht übergeben, damit es nicht in der Shell-History landet.
# NAME statt USER: USER ist im Make immer belegt (der angemeldete Systemnutzer),
# ein vergessenes NAME= würde sonst still am falschen Konto arbeiten.
password: db ## Passwort setzen oder Verwalter anlegen: make password NAME=admin
	@if [ -z "$(NAME)" ]; then \
		echo "Aufruf: make password NAME=admin" >&2; exit 2; \
	fi; \
	cargo run --quiet -- set-password "$(NAME)" --admin

db-shell: ## psql in der laufenden Datenbank
	@$(COMPOSE) exec $(DB_SERVICE) psql -U $(DB_USER) -d autoreverse

db-logs: ## Log der Datenbank
	@$(COMPOSE) logs -f $(DB_SERVICE)

# --- Entwickeln -------------------------------------------------------------

dev: .env db ## Startet den Server auf http://localhost:3000 (Templates sind einkompiliert: nach Änderungen neu starten)
	@cargo run

build: ## Release-Build nach target/release/autoreverse-server
	@cargo build --release

image: ## Baut das Docker-Image autoreverse-server:dev
	@docker build -t autoreverse-server:dev .

stack: .env ## Der komplette Docker-Stack inklusive App auf http://localhost:8180
	@$(COMPOSE) up --build

stack-stop: ## Hält den kompletten Stack an
	@$(COMPOSE) down

stop: ## Hält alles an, was dieses Projekt gestartet hat
	@$(COMPOSE) down

# --- Prüfen -----------------------------------------------------------------

check: ## cargo check für alle Targets
	@cargo check --all-targets

lint: ## cargo fmt --check und clippy mit -D warnings
	@cargo fmt --check
	@cargo clippy --all-targets -- -D warnings

format: ## cargo fmt
	@cargo fmt

test-unit: ## Nur die Unit-Tests im Crate (keine Datenbank nötig)
	@cargo test --lib --bins

test-integration: db ## Integrationstests (eine Wegwerf-Datenbank je Test auf Port 5434)
	@cargo test --test integration

test-js: ## JS-Tests der Browser-Module in static/js
	@node --test 'tests/js/*.test.mjs'

test: db ## Alles außer e2e: lint, cargo test, JS-Tests
	@$(MAKE) --no-print-directory lint
	@cargo test
	@$(MAKE) --no-print-directory test-js

# Nie mit einem Browser auf diesem Mac: das Skript fährt die Tart-VM auf Jarvis hoch.
test-e2e: db ## Playwright in der Tart-VM auf Jarvis: make test-e2e [ARGS=tests/login.e2e.ts]
	@e2e/run-on-jarvis.sh $(ARGS)

# --- Aufräumen --------------------------------------------------------------

clean: ## Entfernt Build-Ausgaben und Testartefakte
	@cargo clean
	@rm -rf e2e/test-results e2e/playwright-report e2e/design/out
	@echo "→ target/, e2e/test-results, e2e/playwright-report, e2e/design/out entfernt."
