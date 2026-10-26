#!/usr/bin/env bash
# The rules' runtime: how the rules' tests (tests/rules/, contract P) reach
# the product — by identifier, over the shell's wire protocol, as a Micro-UI
# does. Installed by `harness init-repo`, complete: it starts the product
# where the layout puts it, and serves nothing while it is not built, so
# every rule's test stays pending until then. Neither a development nor the
# rules' task changes it (the gate `rule-tests-untouched`): a product that
# cannot be started this way contests it, like a rule's test.
#
# What the product provides, by the layout:
#
#   crates/server/        the one binary serving the shell's wire protocol
#                         (POST /capabilities/<id>, POST /commands,
#                         GET /commands/<id>) and GET /health. It reads
#                         PORT, DATABASE_URL, PORTS — `fake`: every port of
#                         ports.json wired to its fake adapter, nothing
#                         reaches outside, no secret needed — and, when set,
#                         FIXTURES_DIR: the mock-up's fixtures
#                         (mock/fixtures/*.json), loaded into its store as it
#                         starts.
#   apps/shell/           the frontend's host: package.json with the scripts
#                         `build` and `serve`; `serve` listens on PORT, reaches
#                         the server at SERVER_URL, and shows a frozen screen
#                         at /screens/<screen>.
#   migrations/*.sql      applied in path order, as scripts/dump-schema.sh does.
#
#   bash scripts/rules-runtime.sh serve
#       Applies the migrations to a scratch database of its own, starts the
#       server on it with the fixtures, then the shell when there is one —
#       in the background, their output in target/rules-runtime/ — and
#       prints what the tests read, one KEY=value a line (the CI appends them
#       to $GITHUB_ENV):
#         RULES_SERVER_URL=http://127.0.0.1:<port>
#         RULES_APP_URL=http://127.0.0.1:<port>
#         RULES_SCREEN_ROUTE=/screens/<screen>
#       Prints nothing while crates/server is not built, or no DATABASE_URL
#       names a Postgres: the tests stay pending. A product that is built
#       and does not start fails: that is a red, not a pending.
#       Never the mock server (mock/src/server.js): it answers the same
#       protocol from the fixtures, and every rule would hold without the
#       product.
#
#   bash scripts/rules-runtime.sh reset
#       Puts the store back as serve left it, for the next rule: stops the
#       server, makes its database again from the migrated template serve
#       kept (rules_seed_<epoch>_<owner>), starts the server on it with the
#       fixtures, on the same port — the shell stays up. Every rule starts
#       on the seed, whatever the rule before it wrote (ForgeDesMondes'
#       PR #597: a business test left a live session open and three journeys
#       followed it; a journey's scenes and deletions broke two more).
#       Fails when serve started nothing.
#
#   bash scripts/rules-runtime.sh stop
#       Stops what serve started, waits for it to exit, and drops its
#       scratch database and its template.
#
# Several runs may share one server (the factory's lanes, each through
# scripts/gates-local.sh): each database is its own,
# rules_runtime_<epoch>_<owner>; RULES_SERVER_PORT and RULES_APP_PORT
# (8790, 8791 by default) are the caller's to keep apart — a server or a
# shell whose port another process answers on fails, never judged against
# the other's; a CREATE DATABASE that meets template1 in use (another
# run's, an autovacuum) is tried again.
# With RULES_RUNTIME_OWNER — the pid of the process the runtime serves —
# the database is named after that process, and a watchdog stops the
# runtime once it is gone, killed outright included: no server and no
# database outlive their owner (the watch's sweep drops what a watchdog
# killed with it left).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
state="$root/target/rules-runtime"
SERVER_PORT="${RULES_SERVER_PORT:-8790}"
APP_PORT="${RULES_APP_PORT:-8791}"
WAIT_SECONDS="${RULES_WAIT_SECONDS:-120}"
CREATE_TRIES="${RULES_CREATE_TRIES:-10}"

say() { echo "rules-runtime: $*" >&2; }

# `pid` and every process under it, one a line.
tree() {
  local pid
  for pid in "$@"; do
    echo "$pid"
    # shellcheck disable=SC2046 # one pid a word
    tree $(pgrep -P "$pid" 2>/dev/null || true)
  done
}

# Whether `port` is `pid`'s, or one of its children's: another lane handed
# the same free port answers there too. Without lsof, or when lsof sees no
# listener, only whether `pid` still runs.
holds() {
  local pid="$1" port="$2" holders
  kill -0 "$pid" 2>/dev/null || return 1
  command -v lsof >/dev/null 2>&1 || return 0
  holders="$(lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true)"
  [ -n "$holders" ] || return 0
  grep -qxF -f <(tree "$pid") <<< "$holders"
}

# Answers once `url` does, within WAIT_SECONDS, from `pid` — started to
# listen on `port`; fails otherwise: `pid` gone before it answered, or
# another process answering on its port, is a port taken, never a verdict
# read off someone else's product.
wait_for() {
  local url="$1" what="$2" pid="$3" port="$4" waited=0
  until curl -fsS -o /dev/null "$url" 2>/dev/null; do
    if ! kill -0 "$pid" 2>/dev/null; then
      say "$what exited before it answered (port $port taken?) — see $state/"
      return 1
    fi
    if [ "$waited" -ge "$WAIT_SECONDS" ]; then
      say "$what does not answer at $url after ${WAIT_SECONDS}s — see $state/"
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
  if ! holds "$pid" "$port"; then
    say "$what does not hold port $port: another process answers there — see $state/"
    return 1
  fi
}

# CREATE DATABASE `name`, tried again while template1 is in use — the
# server refuses a copy of the template while anyone else is on it.
create_database() {
  local name="$1" tries=1 err
  until err="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -qAt -c "CREATE DATABASE $name" 2>&1 >/dev/null)"; do
    if [ "$tries" -ge "$CREATE_TRIES" ] || ! grep -q "is being accessed by other users" <<< "$err"; then
      say "CREATE DATABASE $name failed after $tries attempt(s): $err"
      return 1
    fi
    tries=$((tries + 1))
    sleep 1
  done
}

# The scratch database the migrations build, its URL on stdout.
scratch_database() {
  local url="$DATABASE_URL" query="" server name
  case "$url" in
    *\?*) query="?${url#*\?}"; url="${url%%\?*}" ;;
  esac
  server="${url%/*}"
  name="rules_runtime_$(date +%s)_${RULES_RUNTIME_OWNER:-$$}"
  # A command substitution runs without `set -e`: each failure returns.
  create_database "$name" || return 1
  echo "$name" > "$state/database"
  echo "$DATABASE_URL" > "$state/admin-url"
  local target="$server/$name$query"
  local file
  while IFS= read -r file; do
    if ! psql "$target" -v ON_ERROR_STOP=1 -qAt -f "$root/$file" >/dev/null 2>"$state/migration.err"; then
      say "migration $file failed: $(tail -n 1 "$state/migration.err")"
      return 1
    fi
  done < <(cd "$root" && git ls-files -- '*migrations/*.sql' | LC_ALL=C sort)
  # The migrated store, before anyone connects: the template `reset` copies.
  local seed="${name/rules_runtime_/rules_seed_}"
  create_database "$seed TEMPLATE $name" || return 1
  echo "$seed" > "$state/seed"
  echo "$target" > "$state/database-url"
  echo "$target"
}

# Starts the server on the scratch database, with the fixtures, and waits
# for it: what serve does once, and reset after every rule.
start_server() {
  local fixtures=""
  [ -d "$root/mock/fixtures" ] && fixtures="$root/mock/fixtures"
  # DATABASE_URL_READONLY emptied: the session's own would have the server
  # read the dev database while it writes the scratch one.
  PORT="$SERVER_PORT" DATABASE_URL="$(cat "$state/database-url")" DATABASE_URL_READONLY='' \
    FIXTURES_DIR="$fixtures" PORTS=fake \
    nohup "$(cat "$state/binary")" >>"$state/server.log" 2>&1 &
  echo $! > "$state/server.pid"
  wait_for "http://127.0.0.1:$SERVER_PORT/health" "the server" "$(cat "$state/server.pid")" "$SERVER_PORT"
}

# Stops the process whose pid `file` holds (and its children), and waits.
stop_pid() {
  local file="$1" pid waited=0
  [ -f "$file" ] || return 0
  pid="$(cat "$file")"
  pkill -P "$pid" 2>/dev/null || true
  kill "$pid" 2>/dev/null || true
  while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 20 ]; do
    sleep 0.5
    waited=$((waited + 1))
  done
  kill -9 "$pid" 2>/dev/null || true
  rm -f "$file"
}

# DROP DATABASE `name`, whoever is still on it cut off (PostgreSQL 13+).
drop_database() {
  local admin="$1" name="$2"
  psql "$admin" -qAt -c "DROP DATABASE IF EXISTS $name WITH (FORCE)" >/dev/null 2>&1 \
    || psql "$admin" -qAt -c "DROP DATABASE IF EXISTS $name" >/dev/null 2>&1
}

reset() {
  # Read off what serve kept of the store, never off the server's pid: a
  # reset that failed half-way (the server stopped, the store not made
  # again) is mended by the next one, not refused.
  if [ ! -f "$state/seed" ] || [ ! -f "$state/database" ] || [ ! -f "$state/binary" ]; then
    say "reset: serve started nothing to reset"
    return 1
  fi
  stop_pid "$state/server.pid"
  local admin database seed
  admin="$(cat "$state/admin-url")"
  database="$(cat "$state/database")"
  seed="$(cat "$state/seed")"
  DATABASE_URL="$admin" drop_database "$admin" "$database" || {
    say "reset: the database $database could not be dropped"
    return 1
  }
  DATABASE_URL="$admin" create_database "$database TEMPLATE $seed" || return 1
  start_server
}

serve() {
  if [ ! -f "$root/crates/server/Cargo.toml" ]; then
    say "no crates/server yet: nothing to serve, the rules' tests stay pending"
    return 0
  fi
  if [ -z "${DATABASE_URL:-}" ]; then
    say "no DATABASE_URL: no store to serve on, the rules' tests stay pending"
    return 0
  fi
  mkdir -p "$state"

  (cd "$root" && cargo build --quiet --manifest-path crates/server/Cargo.toml)
  local binary
  binary="$(cd "$root" && cargo metadata --format-version 1 --no-deps --manifest-path crates/server/Cargo.toml \
    | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const m=JSON.parse(s);const p=m.packages.find(p=>p.manifest_path.endsWith("crates/server/Cargo.toml"));const b=p&&p.targets.find(t=>t.kind.includes("bin"));process.stdout.write(`${m.target_directory}/debug/${b?b.name:""}`)})')"
  if [ ! -x "$binary" ]; then
    say "crates/server builds no binary ($binary)"
    return 1
  fi
  # Created after the build, right before the server connects to it: a
  # database left idle through a cold build would look stale to the watch's
  # sweep (idle, dated, half an hour old) and be dropped under the server.
  local database
  database="$(scratch_database)"
  # The runtime dies with its owner: once RULES_RUNTIME_OWNER is gone, the
  # watchdog stops what serve started and drops the database.
  if [ -n "${RULES_RUNTIME_OWNER:-}" ]; then
    (while kill -0 "$RULES_RUNTIME_OWNER" 2>/dev/null; do sleep 2; done; stop watchdog) </dev/null >/dev/null 2>&1 &
    echo $! > "$state/watchdog.pid"
  fi
  echo "$binary" > "$state/binary"
  : > "$state/server.log"
  start_server
  local server_url="http://127.0.0.1:$SERVER_PORT"
  echo "RULES_SERVER_URL=$server_url"

  if [ -f "$root/apps/shell/package.json" ]; then
    (cd "$root/apps/shell" && npm install --no-audit --no-fund >"$state/shell-install.log" 2>&1 && npm run build >"$state/shell-build.log" 2>&1)
    # The list gets its own redirections: a backgrounded `( … )` keeps the
    # script's stdout open, and `serve | tee` would never see end-of-file.
    (cd "$root/apps/shell" && PORT="$APP_PORT" SERVER_URL="$server_url" exec nohup npm run serve >"$state/shell.log" 2>&1) </dev/null >/dev/null 2>&1 &
    echo $! > "$state/shell.pid"
    local app_url="http://127.0.0.1:$APP_PORT"
    wait_for "$app_url/" "the shell" "$(cat "$state/shell.pid")" "$APP_PORT"
    echo "RULES_APP_URL=$app_url"
    echo "RULES_SCREEN_ROUTE=/screens/<screen>"
  fi
}

# Stops what serve started — the watchdog first, unless it is the one
# stopping (`stop watchdog`) — waits for it to exit, then drops the
# database: a server still connected would keep it.
stop() {
  local pid waited=0 pids=()
  if [ "${1:-}" != watchdog ] && [ -f "$state/watchdog.pid" ]; then
    kill "$(cat "$state/watchdog.pid")" 2>/dev/null || true
  fi
  rm -f "$state/watchdog.pid"
  for name in shell server; do
    if [ -f "$state/$name.pid" ]; then
      pid="$(cat "$state/$name.pid")"
      # npm runs the shell as its child: both go.
      pkill -P "$pid" 2>/dev/null || true
      kill "$pid" 2>/dev/null || true
      pids+=("$pid")
      rm -f "$state/$name.pid"
    fi
  done
  for pid in ${pids[@]+"${pids[@]}"}; do
    while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 20 ]; do
      sleep 0.5
      waited=$((waited + 1))
    done
    kill -9 "$pid" 2>/dev/null || true
  done
  if [ -f "$state/database" ] && [ -f "$state/admin-url" ]; then
    local admin database
    admin="$(cat "$state/admin-url")"
    database="$(cat "$state/database")"
    drop_database "$admin" "$database" \
      || say "the database $database could not be dropped: the watch's sweep will"
    if [ -f "$state/seed" ]; then
      drop_database "$admin" "$(cat "$state/seed")" \
        || say "the template $(cat "$state/seed") could not be dropped: the watch's sweep will"
    fi
    rm -f "$state/database" "$state/admin-url" "$state/seed" "$state/database-url"
  fi
}

case "${1:-}" in
  serve) serve ;;
  reset) reset ;;
  stop) stop ;;
  *)
    echo "usage: bash scripts/rules-runtime.sh serve | reset | stop" >&2
    exit 2
    ;;
esac
