#!/usr/bin/env bash
# Local dev loop: spins up a throwaway Postgres for directory, runs
# directory (dev pepper + dev OTP vendor, code is always "000000"), then
# launches the desktop client. Ctrl-C tears down directory + the client;
# the DB container is left running (fast restart next time) — remove it
# with `docker rm -f chat-directory-devdb` if you want a clean slate.
set -euo pipefail
cd "$(dirname "$0")/.."

# ponytail: hardcoded local dev JDK/SDK paths (this machine only — see the
# T27 build session). Point elsewhere via env if your machine differs.
export JAVA_HOME="${JAVA_HOME:-/home/justfossa/.jdks/temurin-17}"
export ANDROID_HOME="${ANDROID_HOME:-/home/justfossa/Android/Sdk}"

DB_PORT=5433
DB_CONTAINER=chat-directory-devdb

if ! docker inspect "$DB_CONTAINER" >/dev/null 2>&1; then
    echo "==> starting $DB_CONTAINER (postgres:16-alpine on :$DB_PORT)"
    docker run -d --name "$DB_CONTAINER" \
        -e POSTGRES_USER=directory -e POSTGRES_PASSWORD=devpassword -e POSTGRES_DB=directory \
        -p "$DB_PORT:5432" postgres:16-alpine >/dev/null
elif [ "$(docker inspect -f '{{.State.Running}}' "$DB_CONTAINER")" != "true" ]; then
    echo "==> restarting $DB_CONTAINER"
    docker start "$DB_CONTAINER" >/dev/null
fi

export DATABASE_URL="postgres://directory:devpassword@localhost:$DB_PORT/directory"
export DIRECTORY_ALLOW_DEV_PEPPER=1
export DIRECTORY_ALLOW_DEV_OTP_VENDOR=1
export DIRECTORY_ACCOUNTS_ENABLED=1
export DIRECTORY_SEARCH_ENABLED=1

echo "==> waiting for postgres"
until docker exec "$DB_CONTAINER" pg_isready -U directory >/dev/null 2>&1; do sleep 0.5; done

echo "==> starting directory (127.0.0.1:7444, migrations run automatically on connect)"
cargo run -p directory &
DIRECTORY_PID=$!
trap 'kill "$DIRECTORY_PID" 2>/dev/null || true' EXIT

until curl -sf http://127.0.0.1:7444/health >/dev/null 2>&1; do
    sleep 0.5
    kill -0 "$DIRECTORY_PID" 2>/dev/null || { echo "directory exited early"; exit 1; }
done
echo "==> directory is up"

echo "==> launching desktop client (dev OTP vendor accepts code 000000 for any number)"
cd app && ./gradlew :composeApp:run
