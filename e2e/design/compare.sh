#!/usr/bin/env bash
# Design parity check: runs the legacy SvelteKit server and the Rust server side by side on this Mac with identical data, screenshots every page of both in the Tart
# VM on Jarvis (never a browser on this Mac) and computes pixel diffs here.
#
#   e2e/design/compare.sh                  # all shots
#   SHOTS=books,login e2e/design/compare.sh
#
# The legacy server is no longer in the tree. The script exports it from git (LEGACY_REV, default
# 0b627bd — the last commit before the Rust rewrite) into $LEGACY_DIR and builds it there once;
# later runs reuse that build as long as the revision matches.
# Needs: node/npm on PATH (bun is used instead when installed, it honours legacy's bun.lock).
# Output: $DESIGN_OUT (default e2e/design/out): <shot>__<viewport>__<theme>__{legacy,rust,diff}.png
# and report.txt with the share of differing pixels per pair.
#
# Env: E2E_KEEP_VM=1, E2E_KEEP_SERVERS=1 (leave both running, e.g. while iterating on CSS —
#      templates are compiled in, so Rust changes need a restart anyway), DESIGN_SKIP_SETUP=1
#      (servers already running from a previous E2E_KEEP_SERVERS run), DESIGN_SETUP_ONLY=1 (start
#      and fill both servers, leave them running, no VM).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
E2E="$ROOT/e2e"
RUST_PORT=3100
LEGACY_PORT=3200
FEEDS_PORT=3101  # e2e/fixtures/podcast/feed.xml names it
RUST_DB=autoreverse_design
LEGACY_DB=autoreverse_legacy
VM="${E2E_VM:-autoreverse-e2e}"
JARVIS="${E2E_JARVIS:-jarvis}"
OUT="${DESIGN_OUT:-$E2E/design/out}"
STATE="${TMPDIR:-/tmp}/autoreverse-design"
LEGACY_REV="${LEGACY_REV:-0b627bd}"
LEGACY_DIR="${LEGACY_DIR:-$STATE/legacy-src}"
if [[ -z "${E2E_HOST_IP:-}" ]]; then
	iface="$(route -n get default 2>/dev/null | awk '/interface:/ {print $2}')"
	E2E_HOST_IP="$(ipconfig getifaddr "${iface:-en0}")"
fi
VM_DIR='~/e2e/design'
VM_STARTED=''

log() { printf '\033[1m[design]\033[0m %s\n' "$*"; }
psql_db() { docker compose -f "$ROOT/compose.yaml" exec -T postgres psql -v ON_ERROR_STOP=1 -q -U autoreverse "$@"; }
vm() {
	local i=''
	[[ "$1" == -i ]] && { i='-i'; shift; }
	ssh "$JARVIS" "vm-exec $VM $i $(printf '%q' "$1")"
}
stop_servers() {
	for f in "$STATE"/*.pid; do
		[[ -f "$f" ]] || continue
		kill "$(cat "$f")" 2>/dev/null || true
		rm -f "$f"
	done
}
cleanup() {
	local status=$?
	if [[ -n "$VM_STARTED" && -z "${E2E_KEEP_VM:-}" ]]; then
		log "VM $VM herunterfahren"
		ssh "$JARVIS" "vm-down $VM" || true
	fi
	[[ -z "${E2E_KEEP_SERVERS:-}" ]] && stop_servers
	exit "$status"
}
trap cleanup EXIT INT TERM

wait_http() {
	for _ in $(seq 1 120); do
		curl -sf -o /dev/null "$1" && return 0
		sleep 0.5
	done
	echo "nicht erreichbar: $1" >&2
	exit 1
}

# token <port>: logs in as Oliver over the API.
token() {
	curl -sf -X POST -H 'content-type: application/json' \
		-d '{"name":"Oliver","password":"hunter2hunter2"}' "http://127.0.0.1:$1/auth/login" |
		python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])'
}

# populate <port> <db>: seed, library roots, scan, podcast — the same steps for both servers so
# both databases hold the same rows in the same order.
populate() {
	local port=$1 db=$2 tok
	psql_db -d "$db" <"$E2E/seed.sql"
	psql_db -d "$db" <"$E2E/design/seed.sql"
	psql_db -d "$db" -c "INSERT INTO library_config (id, books_dir, music_dir) VALUES (1,
		'$ROOT/tests/fixtures/library/books', '$ROOT/tests/fixtures/library/music')
		ON CONFLICT (id) DO UPDATE SET books_dir = EXCLUDED.books_dir, music_dir = EXCLUDED.music_dir"
	tok="$(token "$port")"
	curl -sf -o /dev/null -X POST -H "authorization: Bearer $tok" "http://127.0.0.1:$port/scan"
	for _ in $(seq 1 60); do
		curl -sf -H "authorization: Bearer $tok" "http://127.0.0.1:$port/scan/status" |
			grep -q '"running":true' || break
		sleep 0.5
	done
	curl -sf -o /dev/null -X POST -H "authorization: Bearer $tok" -H 'content-type: application/json' \
		-d "{\"feed_url\":\"http://127.0.0.1:$FEEDS_PORT/feed.xml\"}" "http://127.0.0.1:$port/podcasts"
}

# fetch_legacy: git archive of LEGACY_REV into LEGACY_DIR, installed and built, unless a build of
# exactly that revision is already there.
fetch_legacy() {
	local rev
	rev="$(git -C "$ROOT" rev-parse --verify "$LEGACY_REV^{commit}")"
	if [[ -d "$LEGACY_DIR/build" && "$(cat "$LEGACY_DIR/.rev" 2>/dev/null)" == "$rev" ]]; then
		return
	fi
	log "Legacy-Stand ${rev:0:7} nach $LEGACY_DIR holen und bauen"
	rm -rf "$LEGACY_DIR"
	mkdir -p "$LEGACY_DIR"
	git -C "$ROOT" archive "$rev" | tar -C "$LEGACY_DIR" -xf -
	(
		cd "$LEGACY_DIR"
		export DATABASE_URL="postgresql://autoreverse:autoreverse@localhost:5434/$LEGACY_DB"
		if command -v bun >/dev/null; then
			bun install --frozen-lockfile && bun run build
		else
			npm install --no-audit --no-fund && npm run build
		fi
	) >"$STATE/legacy-build.log" 2>&1 || { log "Legacy-Build fehlgeschlagen, siehe $STATE/legacy-build.log"; exit 1; }
	echo "$rev" >"$LEGACY_DIR/.rev"
}

if [[ -z "${DESIGN_SKIP_SETUP:-}" ]]; then
	stop_servers
	mkdir -p "$STATE"
	fetch_legacy
	log "Rust-Server bauen"
	cargo build --release --manifest-path "$ROOT/Cargo.toml" --quiet

	log "Datenbanken neu anlegen"
	psql_db -d autoreverse -c "DROP DATABASE IF EXISTS $RUST_DB WITH (FORCE)" -c "CREATE DATABASE $RUST_DB" \
		-c "DROP DATABASE IF EXISTS $LEGACY_DB WITH (FORCE)" -c "CREATE DATABASE $LEGACY_DB"
	(cd "$LEGACY_DIR" && DATABASE_URL="postgresql://autoreverse:autoreverse@localhost:5434/$LEGACY_DB" \
		npx drizzle-kit push --force >/dev/null)

	log "Podcast-Fixtures auf 127.0.0.1:$FEEDS_PORT"
	rm -rf "$STATE/feeds" "$STATE/rust" "$STATE/legacy"
	mkdir -p "$STATE/feeds" "$STATE/rust" "$STATE/legacy"
	cp "$E2E/fixtures/podcast/"* "$STATE/feeds/"
	for ep in ep1 ep2; do
		ffmpeg -y -loglevel error -f lavfi -i anullsrc=r=8000:cl=mono -t 5 "$STATE/feeds/$ep.mp3"
	done
	python3 -m http.server "$FEEDS_PORT" --bind 127.0.0.1 --directory "$STATE/feeds" >"$STATE/feeds.log" 2>&1 &
	echo $! >"$STATE/feeds.pid"

	common=(AUTOREVERSE_ADMIN_USER=Oliver AUTOREVERSE_ADMIN_PASSWORD=hunter2hunter2
		AUTOREVERSE_PODCAST_REFRESH_HOURS=0 HOST=0.0.0.0)
	log "Rust-Server auf :$RUST_PORT"
	ln -sfn "$ROOT/static" "$STATE/rust/static"
	(cd "$STATE/rust" && exec env "${common[@]}" PORT=$RUST_PORT AUTOREVERSE_DATA="$STATE/rust/data" \
		DATABASE_URL="postgresql://autoreverse:autoreverse@localhost:5434/$RUST_DB" \
		AUTOREVERSE_PODCAST_DIRECTORY_URL="http://127.0.0.1:$FEEDS_PORT/search" \
		"$ROOT/target/release/autoreverse-server" >"$STATE/rust.log" 2>&1) &
	echo $! >"$STATE/rust.pid"
	log "Legacy-Server auf :$LEGACY_PORT"
	(cd "$STATE/legacy" && exec env "${common[@]}" PORT=$LEGACY_PORT AUTOREVERSE_DATA="$STATE/legacy/data" \
		ORIGIN="http://$E2E_HOST_IP:$LEGACY_PORT" \
		DATABASE_URL="postgresql://autoreverse:autoreverse@localhost:5434/$LEGACY_DB" \
		node "$LEGACY_DIR/build" >"$STATE/legacy.log" 2>&1) &
	echo $! >"$STATE/legacy.pid"
	wait_http "http://127.0.0.1:$RUST_PORT/login"
	wait_http "http://127.0.0.1:$LEGACY_PORT/login"

	log "Daten anlegen (Seed, Scan, Podcast)"
	populate $LEGACY_PORT $LEGACY_DB
	populate $RUST_PORT $RUST_DB
fi
if [[ -n "${DESIGN_SETUP_ONLY:-}" ]]; then
	log "Server laufen: legacy :$LEGACY_PORT, rust :$RUST_PORT (stoppen: kill \$(cat $STATE/*.pid))"
	E2E_KEEP_SERVERS=1
	exit 0
fi

log "VM $VM starten"
ssh "$JARVIS" "vm-up --wait $VM"
VM_STARTED=1
ssh "$JARVIS" "until vm-exec $VM 'pgrep -x Dock' >/dev/null; do sleep 2; done"
for port in $RUST_PORT $LEGACY_PORT; do
	vm "curl -sf -m 10 -o /dev/null http://$E2E_HOST_IP:$port/login" ||
		{ log "VM erreicht :$port nicht (Firewall?)"; exit 1; }
done

log "Spec kopieren und ausführen"
vm "rm -rf $VM_DIR && mkdir -p $VM_DIR && ln -s ~/e2e/node_modules $VM_DIR/node_modules"
tar -C "$E2E/design" -cf - playwright.config.ts compare.spec.ts | vm -i "tar -C $VM_DIR -xf -"
set +e
vm "cd $VM_DIR && LEGACY_URL=http://$E2E_HOST_IP:$LEGACY_PORT RUST_URL=http://$E2E_HOST_IP:$RUST_PORT \
	SHOTS=$(printf '%q' "${SHOTS:-}") VIEWPORTS=$(printf '%q' "${VIEWPORTS:-}") THEMES=$(printf '%q' "${THEMES:-}") \
	~/e2e/node_modules/.bin/playwright test"
result=$?
set -e

log "Screenshots holen und vergleichen"
mkdir -p "$OUT"
[[ -z "${SHOTS:-}${VIEWPORTS:-}${THEMES:-}" ]] && rm -f "$OUT"/*.png
vm "cd $VM_DIR/shots && tar -cf - ." | tar -C "$OUT" -xf -
node "$E2E/design/diff.mjs" "$OUT"
exit "$result"
