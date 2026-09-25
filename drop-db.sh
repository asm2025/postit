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
# Connection: ADMIN_DATABASE_URL env var, else !ref/vault/<environment>/postit-database-url.txt,
# else (development only) the fixed dev URL from server/config/development.toml.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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
    echo "$environment requires --no-stop: stop 'api' and 'worker' in" >&2
    echo "docker/docker-compose.$environment.yaml (or scale them to zero) yourself first," >&2
    echo "then re-run with --no-stop." >&2
    exit 1
fi

# --- Resolve the connection ---

vault_file="$script_dir/!ref/vault/$environment/postit-database-url.txt"
if [ -n "${ADMIN_DATABASE_URL:-}" ]; then
    db_url="$ADMIN_DATABASE_URL"
elif [ -f "$vault_file" ]; then
    db_url="$(cat "$vault_file")"
elif [ "$environment" = "development" ]; then
    db_url="postgres://postit:postit@localhost:44340/postit"
else
    echo "No vault file at $vault_file and no ADMIN_DATABASE_URL set." >&2
    echo "Extract the vault first (see README: 'Extract configuration files')." >&2
    exit 1
fi

if [[ "$db_url" =~ ^postgres(ql)?://([^:@/]+):([^@]*)@([^:/]+):([0-9]+)/([^?]+) ]]; then
    db_user="${BASH_REMATCH[2]}"
    db_password="${BASH_REMATCH[3]}"
    db_host="${BASH_REMATCH[4]}"
    db_port="${BASH_REMATCH[5]}"
    db_name="${BASH_REMATCH[6]}"
else
    echo "Could not parse database URL (expected postgres://user:password@host:port/db)." >&2
    exit 1
fi

export PGPASSWORD="$db_password"
maintenance_url="postgres://$db_user@$db_host:$db_port/postgres"

# --- Stop the migrating processes first ---

docker_dir="$script_dir/docker"
compose=(docker compose --project-directory "$docker_dir" -f "$docker_dir/docker-compose.yaml" -f "$docker_dir/docker-compose.dev.yaml")
stopped_services=""

# The app-profile services that connect to Postgres (server, worker from plan 02 P6/P7
# on). Until they exist, this is a no-op — nginx-app is also in the app profile but never
# connects to Postgres directly, so it is not a migrating process and stays running.
migrating_service_candidates="server worker"

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

echo "Dropping database '$db_name' on $db_host:$db_port..."
psql "$maintenance_url" -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS \"$db_name\" WITH (FORCE);"
psql "$maintenance_url" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$db_name\" OWNER \"$db_user\";"
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
