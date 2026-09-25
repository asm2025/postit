#!/usr/bin/env bash
# Resets an environment's postit database to empty. Postgres only — no ECS, no bastion,
# no EF-style scaffolding. The server recreates the schema on its next boot.
#
# Usage: ./drop-db.sh <development|qa|production> [--force] [--no-stop]
#   --force    skip the retype-to-confirm prompt (development, qa only; production
#              always prompts and refuses --force)
#   --no-stop  assert the operator already stopped the migrating processes; required
#              for qa/production, optional for development (skips the app-service stop
#              and the "stop your native cargo run" reminder)
#
# Connection: the postit-postgres container on this host, as its own POSTGRES_USER (from
# the vault) over the local socket, which the postgres image trusts — so no password is
# handled here and no host psql is needed. Set ADMIN_DATABASE_URL
# (postgres://user:password@host:port/db) to reset a database elsewhere with a host psql.

set -euo pipefail

# `pwd -W` (Git Bash / MSYS only) yields D:/... instead of /d/..., which a Windows
# docker.exe would otherwise read as a path on the current drive (D:\d\...).
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && { pwd -W 2>/dev/null || pwd; })"

environment=""
force=0
no_stop=0

for arg in "$@"; do
    case "$arg" in
        development|qa|production) environment="$arg" ;;
        --force) force=1 ;;
        --no-stop) no_stop=1 ;;
        *)
            echo "Unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

if [ -z "$environment" ]; then
    echo "Usage: $0 <development|qa|production> [--force] [--no-stop]" >&2
    exit 1
fi

if [ "$environment" = "production" ] && [ "$force" = 1 ]; then
    echo "production cannot be forced." >&2
    exit 1
fi

if [ "$environment" != "development" ] && [ "$no_stop" = 0 ]; then
    echo "$environment requires --no-stop: stop postit-api and postit-worker yourself first" >&2
    echo "(docker stop postit-api postit-worker), then re-run with --no-stop." >&2
    exit 1
fi

# --- Resolve the connection ---

if [ -n "${ADMIN_DATABASE_URL:-}" ]; then
    if [[ "$ADMIN_DATABASE_URL" =~ ^postgres(ql)?://([^:@/]+):([^@]*)@([^:/]+):([0-9]+)/([^?]+) ]]; then
        export PGPASSWORD="${BASH_REMATCH[3]}"
        target="${BASH_REMATCH[4]}:${BASH_REMATCH[5]}"
        db_name="${BASH_REMATCH[6]}"
        maintenance_url="postgres://${BASH_REMATCH[2]}@$target/postgres"
    else
        echo "Could not parse ADMIN_DATABASE_URL (expected postgres://user:password@host:port/db)." >&2
        exit 1
    fi
else
    # The database name is config (database.url in server/config/<environment>.toml),
    # the same in every environment.
    db_name="postit"
    target="the postit-postgres container"
    if [ "$(docker inspect --format '{{.State.Running}}' postit-postgres 2>/dev/null)" != "true" ]; then
        echo "postit-postgres is not running on this host. Start it (./stack.sh up $environment)," >&2
        echo "or set ADMIN_DATABASE_URL to reach a database elsewhere." >&2
        exit 1
    fi
fi

# Runs one statement against the maintenance database. The statement goes to the
# container as a positional argument, so no shell ever re-parses its quotes.
run_sql() {
    if [ -n "${ADMIN_DATABASE_URL:-}" ]; then
        psql "$maintenance_url" -v ON_ERROR_STOP=1 -c "$1"
    else
        docker exec postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d postgres -v ON_ERROR_STOP=1 -c "$1"' sh "$1"
    fi
}

# --- Stop the migrating processes first ---

docker_dir="$script_dir/docker"
compose=(docker compose -f "$docker_dir/docker-compose.yml" -f "$docker_dir/docker-compose.development.yml")
stopped_services=""

# The app-profile services that connect to Postgres (postit-server from plan 02 P6/P7
# on). Until they exist, this is a no-op — postit-nginx-app is also in the app profile but
# never connects to Postgres directly, so it is not a migrating process and stays running.
migrating_service_candidates="postit-server postit-api postit-worker"

if [ "$no_stop" = 0 ]; then
    if [ "$environment" = "development" ]; then
        all_services="$("${compose[@]}" config --services 2>/dev/null || true)"
        app_services=""
        for s in $migrating_service_candidates; do
            echo "$all_services" | grep -qx "$s" && app_services="$app_services $s"
        done
        app_services="$(echo "$app_services" | xargs -n1 2>/dev/null || true)"
        if [ -n "$app_services" ]; then
            echo "Stopping app services: $app_services"
            # shellcheck disable=SC2086
            "${compose[@]}" stop $app_services
            stopped_services="$app_services"
        fi
        if [ "$force" = 0 ]; then
            echo "If a native 'cargo run' server is running against this database, stop it now."
            read -r -p "Press Enter to continue..." _ || true
        fi
    fi
fi

# --- Confirm ---

if [ "$environment" = "production" ] || [ "$force" = 0 ]; then
    read -r -p "Type the database name ('$db_name') to confirm dropping it: " typed
    if [ "$typed" != "$db_name" ]; then
        echo "Confirmation did not match. Aborting." >&2
        exit 1
    fi
fi

# --- Drop and recreate ---

echo "Dropping database '$db_name' on $target..."
run_sql "DROP DATABASE IF EXISTS \"$db_name\" WITH (FORCE);"
# No OWNER clause: the connecting role owns it, as it owned the one just dropped.
run_sql "CREATE DATABASE \"$db_name\";"
echo "Database '$db_name' dropped and recreated empty."

# --- Restart ---

if [ -n "$stopped_services" ]; then
    echo "Restarting app services: $stopped_services"
    # shellcheck disable=SC2086
    "${compose[@]}" start $stopped_services
    echo "Waiting for app services to come back up..."
    for _ in $(seq 1 30); do
        running="$("${compose[@]}" ps --services --filter "status=running" 2>/dev/null || true)"
        all_up=1
        for s in $stopped_services; do
            echo "$running" | grep -qx "$s" || all_up=0
        done
        [ "$all_up" = 1 ] && break
        sleep 2
    done
    if [ "$all_up" != 1 ]; then
        echo "Warning: not all app services reported running after 60s; check 'docker compose ps'." >&2
    fi
fi
