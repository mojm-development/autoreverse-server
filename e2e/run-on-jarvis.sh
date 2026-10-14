#!/usr/bin/env bash
# Runs the Playwright e2e specs against a locally running Rust server, with the browser in the
# Tart VM "autoreverse-e2e" on Jarvis — never on this Mac (global rule: no UI tests here).
#
#   e2e/run-on-jarvis.sh                     # all specs
#   e2e/run-on-jarvis.sh tests/login.e2e.ts  # extra args go to `playwright test`
#
# Steps: build the server, recreate the database $E2E_DB on the dev Postgres (:5434, compose
# service "postgres"), start the server on 0.0.0.0:$E2E_PORT with "Oliver" as first admin, apply
# e2e/seed.sql, boot the VM, copy e2e/ into it, run Playwright, fetch the HTML report into
# e2e/playwright-report, stop the VM and the server (also on failure/Ctrl-C).
#
# Env: E2E_HOST_IP (this Mac as the VM sees it; default: IP of the default-route interface),
#      E2E_KEEP_VM=1 leaves the VM running for the next iteration (stop it with
#      `ssh jarvis vm-down autoreverse-e2e`), E2E_PORT (3100), E2E_DB (autoreverse_e2e), E2E_VM (autoreverse-e2e), E2E_JARVIS (jarvis).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
E2E="$ROOT/e2e"
PORT="${E2E_PORT:-3100}"
DB="${E2E_DB:-autoreverse_e2e}"
VM="${E2E_VM:-autoreverse-e2e}"
JARVIS="${E2E_JARVIS:-jarvis}"
if [[ -z "${E2E_HOST_IP:-}" ]]; then
	iface="$(route -n get default 2>/dev/null | awk '/interface:/ {print $2}')"
	E2E_HOST_IP="$(ipconfig getifaddr "${iface:-en0}")"
fi
BASE_URL="http://$E2E_HOST_IP:$PORT"
VM_DIR='~/e2e/autoreverse'   # ~/e2e/node_modules in the VM holds @playwright/test + Chromium
WORK="$(mktemp -d "${TMPDIR:-/tmp}/autoreverse-e2e.XXXXXX")"
SERVER_PID=''
VM_STARTED=''

log() { printf '\033[1m[e2e]\033[0m %s\n' "$*"; }
psql_db() { docker compose -f "$ROOT/compose.yaml" exec -T postgres psql -v ON_ERROR_STOP=1 -q -U autoreverse "$@"; }
# vm '<cmd>' runs one zsh command line in the VM (quoted once for Jarvis' shell, vm-exec hands it
# to zsh -lc). vm -i '<cmd>' passes stdin through.
vm() {
	local i=''
	[[ "$1" == -i ]] && { i='-i'; shift; }
	ssh "$JARVIS" "vm-exec $VM $i $(printf '%q' "$1")"
}

cleanup() {
	local status=$?
	if [[ -n "$VM_STARTED" && -z "${E2E_KEEP_VM:-}" ]]; then
		log "VM $VM herunterfahren"
		ssh "$JARVIS" "vm-down $VM" || true
	fi
	if [[ -n "$SERVER_PID" ]]; then
		log "Server beenden"
		kill "$SERVER_PID" 2>/dev/null || true
		wait "$SERVER_PID" 2>/dev/null || true
	fi
	exit "$status"
}
trap cleanup EXIT INT TERM

log "Server bauen"
cargo build --release --manifest-path "$ROOT/Cargo.toml" --quiet

log "Datenbank $DB neu anlegen"
psql_db -d autoreverse -c "DROP DATABASE IF EXISTS $DB WITH (FORCE)" -c "CREATE DATABASE $DB"

if curl -s -o /dev/null "http://127.0.0.1:$PORT/"; then
	log "Port $PORT ist schon belegt (alter Server?) – Abbruch."
	exit 1
fi
log "Server auf 0.0.0.0:$PORT starten (Log: $WORK/server.log)"
# Runs in $WORK, not the repo root: a developer's .env there must not leak in (ORIGIN, admin, …).
# The server serves ./static relative to its working directory, hence the link.
ln -s "$ROOT/static" "$WORK/static"
(
	cd "$WORK"
	exec env DATABASE_URL="postgresql://autoreverse:autoreverse@localhost:5434/$DB" \
		AUTOREVERSE_DATA="$WORK/data" \
		AUTOREVERSE_ADMIN_USER=Oliver AUTOREVERSE_ADMIN_PASSWORD=hunter2hunter2 \
		AUTOREVERSE_PODCAST_REFRESH_HOURS=0 HOST=0.0.0.0 PORT="$PORT" \
		"$ROOT/target/release/autoreverse-server" >"$WORK/server.log" 2>&1
) &
SERVER_PID=$!
for _ in $(seq 1 60); do
	curl -sf -o /dev/null "http://127.0.0.1:$PORT/login" && break
	kill -0 "$SERVER_PID" 2>/dev/null || { cat "$WORK/server.log"; exit 1; }
	sleep 0.5
done
curl -sf -o /dev/null "http://127.0.0.1:$PORT/login"

log "Fixtures (e2e/seed.sql) anwenden"
psql_db -d "$DB" <"$E2E/seed.sql"

log "VM $VM starten"
ssh "$JARVIS" "vm-up --wait $VM"
VM_STARTED=1
ssh "$JARVIS" "until vm-exec $VM 'pgrep -x Dock' >/dev/null; do sleep 2; done"

log "Erreichbarkeit $BASE_URL aus der VM prüfen"
if ! vm "curl -sf -m 10 -o /dev/null $BASE_URL/login"; then
	log "Die VM erreicht $BASE_URL nicht (Firewall? Server an 0.0.0.0?). Abbruch."
	exit 1
fi

log "Specs in die VM kopieren"
vm "rm -rf $VM_DIR && mkdir -p $VM_DIR"
tar -C "$E2E" --exclude node_modules --exclude playwright-report --exclude test-results \
	--exclude playwright -cf - package.json playwright.config.ts tests |
	vm -i "tar -C $VM_DIR -xf -"

log "Playwright in der VM ausführen"
args="$(printf '%q ' "$@")"  # e.g. tests/login.e2e.ts --grep …
set +e
vm "cd $VM_DIR && mkdir -p playwright/.auth && \
	E2E_BASE_URL=$BASE_URL E2E_BOOKS_DIR=$(printf '%q' "$ROOT/tests/fixtures/library/books") \
	E2E_MUSIC_DIR=$(printf '%q' "$ROOT/tests/fixtures/library/music") \
	~/e2e/node_modules/.bin/playwright test $args"
result=$?
set -e

log "Report nach e2e/playwright-report holen"
rm -rf "$E2E/playwright-report" "$E2E/test-results"
vm "cd $VM_DIR && tar -cf - playwright-report test-results 2>/dev/null" | tar -C "$E2E" -xf - || true
cp "$WORK/server.log" "$E2E/playwright-report/server.log" 2>/dev/null || true

log "Fertig (Playwright exit $result). Report: npx playwright show-report e2e/playwright-report"
exit "$result"
