# Entwicklungsumgebung für Autoreverse Server.
#
# `make setup` einmal, danach `make dev`. Jedes Ziel, das die Datenbank braucht,
# startet sie selbst — es gibt keine Reihenfolge, die man kennen müsste.
#
# Warum `set -a && source .env` statt `include .env`: Make übernähme die
# Anführungszeichen aus der Datei wörtlich, DATABASE_URL käme mit Quotes bei
# drizzle-kit an. Die Shell entfernt sie korrekt.

# macOS liefert GNU Make 3.81 aus, das kennt .ONESHELL noch nicht — jede Zeile eines
# Rezepts läuft in ihrer eigenen Shell. Mehrzeilige Logik steht deshalb als eine
# fortgesetzte Zeile da, statt sich stillschweigend auseinanderreißen zu lassen.
SHELL := /bin/bash
.DEFAULT_GOAL := help

COMPOSE := docker compose
DB_SERVICE := postgres
DB_USER := autoreverse
ENV := set -a && source .env && set +a

.PHONY: help setup dev stop doctor \
        db db-stop db-reset db-shell db-logs studio push generate migrate \
        check lint format test test-unit test-integration test-e2e \
        build preview stack stack-stop clean

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

setup: .env dev-library ## Einmalig: .env, Abhängigkeiten, Datenbank, Schema
	@bun install
	@$(MAKE) --no-print-directory db
	@$(MAKE) --no-print-directory push
	@echo
	@echo "Fertig. Weiter mit: make dev"

doctor: ## Prüft, ob alles da ist, was die Umgebung braucht
	@ok=0; \
	for tool in bun docker ffmpeg ffprobe; do \
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

db-reset: ## Wirft die Datenbank samt Inhalt weg und legt sie neu an
	@read -p "Alle lokalen Daten löschen? [j/N] " answer; \
	if [ "$$answer" != "j" ]; then echo "Abgebrochen."; exit 0; fi; \
	$(COMPOSE) down -v && $(MAKE) --no-print-directory db && $(MAKE) --no-print-directory push

db-shell: ## psql in der laufenden Datenbank
	@$(COMPOSE) exec $(DB_SERVICE) psql -U $(DB_USER) -d autoreverse

db-logs: ## Log der Datenbank
	@$(COMPOSE) logs -f $(DB_SERVICE)

studio: db ## Drizzle Studio im Browser
	@$(ENV) && bun run db:studio

# Für die Entwicklung ist `push` der richtige Weg: die Dev-Datenbank hat keinen
# Migrationsverlauf, `migrate` würde auf ihr über bestehende Tabellen stolpern.
# Fragt nach, wenn eine Änderung Daten kosten könnte — deshalb braucht es ein
# Terminal. In CI: `bunx drizzle-kit push --force`.
push: db ## Schema direkt in die Dev-Datenbank (fragt bei riskanten Änderungen nach)
	@$(ENV) && bun run db:push

generate: ## Migration aus den Schema-Änderungen erzeugen
	@$(ENV) && bun run db:generate

# Für eine Datenbank mit Migrationsverlauf — das Docker-Image tut das beim Start
# selbst (AUTOREVERSE_AUTO_MIGRATE). Auf einer per `push` gepflegten Dev-Datenbank
# schlägt es fehl, und das ist richtig so.
migrate: db ## Migrationen anwenden (Produktion/Docker, nicht die Dev-Datenbank)
	@$(ENV) && bun run db:migrate

# --- Entwickeln -------------------------------------------------------------

dev: db ## Startet den Dev-Server auf http://localhost:5173
	@$(ENV) && bun run dev

build: db ## Produktionsbuild (braucht DATABASE_URL für die Routenanalyse)
	@$(ENV) && bun run build

preview: build ## Baut und serviert auf http://localhost:4173
	@$(ENV) && bun run preview

stack: .env ## Der komplette Docker-Stack inklusive App auf http://localhost:8180
	@$(COMPOSE) up --build

stack-stop: ## Hält den kompletten Stack an
	@$(COMPOSE) down

stop: ## Hält alles an, was dieses Projekt gestartet hat
	@$(COMPOSE) down

# --- Prüfen -----------------------------------------------------------------

check: ## svelte-check / tsc
	@bun run check

lint: ## prettier --check und eslint
	@bun run lint

format: ## prettier --write
	@bun run format

test-unit: ## Nur die reinen Unit-Tests (keine Datenbank nötig)
	@bun run test:unit --run --project server tests/unit
	@bun run test:unit --run --project client

test-integration: ## Integrationstests (startet eigene Container über testcontainers)
	@bun run test:unit --run --project server tests/integration

test: ## Alles außer e2e: unit, integration, check, lint
	@$(MAKE) --no-print-directory check
	@$(MAKE) --no-print-directory lint
	@bun run test:unit --run

test-e2e: db ## Playwright gegen einen echten Build (nutzt die Dev-Datenbank)
	@$(ENV) && bun run test:e2e

# --- Aufräumen --------------------------------------------------------------

clean: ## Entfernt Build-Ausgaben und Testartefakte
	@rm -rf .svelte-kit build test-results playwright-report
	@echo "→ .svelte-kit, build, test-results, playwright-report entfernt."
