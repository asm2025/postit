#!/usr/bin/env bash
# Launcher for the postit Docker stack.
#
# One command in front of every environment, so the compose -f chain can never be half
# typed: the base file plus exactly one environment file, always. Mirrors stack.ps1.
#
# Usage:
#   ./stack.sh <command> [environment] [options] [service...]
#
# Commands:
#   up          Bring the stack up in the background.  Pre-flights the environment's
#               vault files (and, in development, the dev certificate) first
#   down        Stop the stack.  -v / --volumes also DESTROYS the database
#   restart     Restart the stack, or the named services
#   ps          Show container status
#   logs        Follow logs, for the stack or the named services
#   config      Print the resolved compose configuration.  env_file entries stay as
#               paths, so no vault secret reaches the terminal
#   build       Build the postit-server image (qa, production)
#   psql        Open a psql shell on the postit database, as the vault's user
#   reset       down -v, then up.  DESTROYS the database
#   help        Show this help message
#
# Environments:
#   development (default), qa, production
#
# Options:
#   --app          development: also run the `app` profile (postit-nginx-app on
#                  44300/44305/44310).  Leave it off while `cargo run` / `flutter run`
#                  own those ports
#   -v, --volumes  down: also destroy the database volume
#   --force        Required to destroy the production database (down -v, reset)
#
# Examples:
#   ./stack.sh up                        Development infrastructure, the default
#   ./stack.sh up --app                  ...plus the app placeholders
#   ./stack.sh logs development postit-zitadel
#   ./stack.sh down -v                   Development, database destroyed
#   ./stack.sh config qa                 What qa resolves to, vault files by path
#   ./stack.sh reset production --force

set -euo pipefail

# `pwd -W` (Git Bash / MSYS only) yields D:/... instead of /d/..., which a Windows
# docker.exe would otherwise read as a path on the current drive (D:\d\...).
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && { pwd -W 2>/dev/null || pwd; })"
docker_dir="$script_dir/docker"
vault_dir="$script_dir/!ref/vault"
cert_file="$docker_dir/shared/nginx/certs/postit.local.crt"

usage() {
    sed -n '2,/^$/{s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

die() {
    echo "stack: $*" >&2
    exit 1
}

command="${1:-help}"
[ $# -gt 0 ] && shift

environment="development"
case "${1:-}" in
    development|qa|production) environment="$1"; shift ;;
esac

app=0
volumes=0
force=0
extra=()
for arg in "$@"; do
    case "$arg" in
        --app) app=1 ;;
        -v|--volumes) volumes=1 ;;
        --force) force=1 ;;
        # Anything else — service names, compose flags such as --services or --tail=100 —
        # goes to docker compose after the subcommand.
        *) extra+=("$arg") ;;
    esac
done

if [ "$app" = 1 ] && [ "$environment" != "development" ]; then
    die "--app applies to development only; qa and production always run postit-api and postit-worker"
fi

compose=(docker compose
    -f "$docker_dir/docker-compose.yml"
    -f "$docker_dir/docker-compose.$environment.yml")

# Every profile, for the commands that must see every container whether or not it was
# started with --app: stopping, listing and following logs.
compose_all=("${compose[@]}" --profile "*")

compose_up=("${compose[@]}")
[ "$app" = 1 ] && compose_up+=(--profile app)

# One vault file per project. Compose fails on a missing env_file too, but only for the
# first one it meets and without saying where the file comes from.
vault_files() {
    case "$environment" in
        development) echo postgres.env postit.env zitadel.env ;;
        *) echo postgres.env postit.env ;;
    esac
}

preflight() {
    if [ "$environment" = "development" ] && [ ! -f "$cert_file" ]; then
        die "no dev certificate at docker/shared/nginx/certs/postit.local.crt — run ./cert.sh first"
    fi
    local missing=()
    local f
    for f in $(vault_files); do
        [ -f "$vault_dir/$environment/$f" ] || missing+=("!ref/vault/$environment/$f")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "stack: $environment needs these vault files, and they are missing:" >&2
        printf '  %s\n' "${missing[@]}" >&2
        die "extract !ref/vault.7z first (README: 'Environment files and secrets')"
    fi
}

guard_destroy() {
    if [ "$environment" = "production" ] && [ "$force" = 0 ]; then
        die "refusing to destroy the production database without --force"
    fi
}

# The cached Zitadel machine key belongs to the Zitadel instance inside the volume. Once
# the volume is gone, `cargo xtask zitadel-bootstrap` would authenticate with a key the
# new instance has never seen, so it goes with the volume.
forget_machine_key() {
    [ "$environment" = "development" ] || return 0
    local key="$docker_dir/zitadel/machinekey/postit-bootstrap.json"
    if [ -f "$key" ]; then
        rm -f "$key"
        echo "stack: removed the cached Zitadel machine key; re-run 'cargo xtask zitadel-bootstrap' from server/ once Zitadel is up"
    fi
}

case "$command" in
    up)
        preflight
        "${compose_up[@]}" up -d ${extra[@]+"${extra[@]}"}
        ;;
    down)
        if [ "$volumes" = 1 ]; then
            guard_destroy
            "${compose_all[@]}" down -v ${extra[@]+"${extra[@]}"}
            forget_machine_key
        else
            "${compose_all[@]}" down ${extra[@]+"${extra[@]}"}
        fi
        ;;
    restart)
        "${compose_all[@]}" restart ${extra[@]+"${extra[@]}"}
        ;;
    ps)
        "${compose_all[@]}" ps ${extra[@]+"${extra[@]}"}
        ;;
    logs)
        "${compose_all[@]}" logs -f ${extra[@]+"${extra[@]}"}
        ;;
    config)
        "${compose_up[@]}" config --no-env-resolution ${extra[@]+"${extra[@]}"}
        ;;
    build)
        [ "$environment" = "development" ] && die "development builds nothing; the server runs natively (cargo run) until plan 02 P6"
        "${compose[@]}" build ${extra[@]+"${extra[@]}"}
        ;;
    psql)
        # The container's own POSTGRES_USER (from the vault) over the local socket, which
        # the postgres image trusts — no password on the command line or in the shell.
        docker exec -it postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
        ;;
    reset)
        guard_destroy
        preflight
        "${compose_all[@]}" down -v
        forget_machine_key
        "${compose_up[@]}" up -d
        ;;
    help|-h|--help)
        usage
        ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac
