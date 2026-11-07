#!/usr/bin/env bash
# The postit Docker Compose stack, one environment at a time. Twin: compose.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=compose
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
compose - run the postit Docker Compose stack for one environment.

USAGE
  ./compose.sh <command> [environment] [options] [docker compose arguments]

COMMANDS
  up        Check the environment's vault files (and, in development, the dev
            certificate), then start the stack in the background
  down      Stop and remove the containers; --volumes also deletes the database
  restart   Restart the stack, or the named services
  ps        Show container status
  logs      Follow the logs of the stack, or of the named services
  config    Print the resolved configuration; vault files stay paths, so no secret
            is printed
  build     Build the postit-server image (development: add --app)
  help      Show this help (also -h, --help, or no command)

OPTIONS
  environment    development (the default), qa, or production
  --app          development only: also run the app profile (postit-server behind
                 postit-nginx-app on 44310, 44311, 44315). Leave it off while a
                 native cargo run or the web dev server owns those ports
  -v, --volumes  down: also delete the database volume. You type the environment
                 name to confirm; in development the cached Zitadel machine key goes
                 too, so re-run cargo xtask zitadel-bootstrap afterwards
  --force        down --volumes: skip the question in development and qa.
                 Production requires --force and still asks
  Anything else goes to docker compose after the command (service names,
  --tail=100, --services).

EXAMPLES
  ./compose.sh up                       Development infrastructure
  ./compose.sh up --app                 ...plus postit-server behind postit-nginx-app
  ./compose.sh logs development postit-zitadel --tail=100
  ./compose.sh config qa                What qa resolves to, vault files by path
  ./compose.sh down --volumes           Delete the development database; then
  ./compose.sh up                       start again on an empty one

EXIT CODES
  0  success
  1  usage error, a refused guard, or an unconfirmed question
  *  any other code is docker compose's own

MORE
  docs/scripts.md                       every operator script and the first-run order
  docs/contracts/04. infrastructure and tooling.md
EOF
}

command="${1:-help}"
[ $# -gt 0 ] && shift

case "$command" in
    help | -h | --help)
        usage
        exit 0
        ;;
    up | down | restart | ps | logs | config | build) ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac

environment=development
app=0
volumes=0
force=0
extra=()
for arg in "$@"; do
    case "$arg" in
        --app) app=1 ;;
        -v | --volumes) volumes=1 ;;
        --force) force=1 ;;
        # The first other argument may name the environment, before or after the flags.
        # Anything else (service names, compose flags such as --tail=100) goes to
        # docker compose after the subcommand.
        *)
            if [ -z "${env_named:-}" ] && [ ${#extra[@]} -eq 0 ] && [ -n "$(parse_environment "$arg")" ]; then
                environment="$arg"
                env_named=1
            else
                extra+=("$arg")
            fi
            ;;
    esac
done

if [ "$app" = 1 ] && [ "$environment" != development ]; then
    die "--app applies to development only; qa and production always run postit-api and postit-worker"
fi

mapfile -t compose < <(compose_args "$environment")
# Every profile, for the commands that must see every container whether or not it was
# started with --app: stopping, listing, restarting, and following logs.
compose_all=("${compose[@]}" --profile '*')
compose_up=("${compose[@]}")
[ "$app" = 0 ] || compose_up+=(--profile app)

cert_dir="${POSTIT_CERT_DIR:-$POSTIT_ROOT/docker/shared/nginx/certs}"

# Compose fails on a missing env_file too, but only for the first one it meets and
# without saying where the file comes from.
preflight() {
    if [ "$environment" = development ] && [ ! -f "$cert_dir/postit.local.crt" ]; then
        die "no dev certificate at docker/shared/nginx/certs/postit.local.crt; run ./cert.sh first"
    fi
    if [ "$environment" = development ] && [ "$app" = 1 ] && [ ! -f "$cert_dir/postit-dev-ca.crt" ]; then
        die "no dev CA at docker/shared/nginx/certs/postit-dev-ca.crt; run ./cert.sh first"
    fi
    local missing=() f dir
    dir="$(vault_dir)"
    for f in $(vault_files "$environment"); do
        [ -f "$dir/$environment/$f" ] || missing+=("!ref/vault/$environment/$f")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "compose: $environment needs these vault files, and they are missing:" >&2
        printf '  %s\n' "${missing[@]}" >&2
        die "run ./vault.sh unpack first (README: 'Environment files and secrets')"
    fi
}

# Deleting the database volume: development and qa ask unless --force; production
# requires --force and asks anyway.
confirm_volumes() {
    if [ "$environment" = production ] && [ "$force" = 0 ]; then
        die "refusing to delete the production database volume without --force"
    fi
    local running
    running="$(running_environment)"
    if [ -n "$running" ] && [ "$running" != "$environment" ]; then
        die "you asked for $environment, but the running postit-postgres belongs to $running; refusing to delete its volume under $environment's rules"
    fi
    if [ "$environment" = production ] || [ "$force" = 0 ]; then
        confirm "$environment" "Type the environment name ($environment) to delete its database volume: " ||
            die "confirmation did not match; nothing was stopped or deleted"
    fi
}

# The cached Zitadel machine key belongs to the Zitadel instance inside the volume. Once
# the volume is gone, `cargo xtask zitadel-bootstrap` would authenticate with a key the
# new instance has never seen, so it goes with the volume.
forget_machine_key() {
    [ "$environment" = development ] || return 0
    local key="${POSTIT_MACHINE_KEY:-$POSTIT_ROOT/docker/zitadel/machinekey/postit-bootstrap.json}"
    if [ -f "$key" ]; then
        rm -f "$key"
        say "removed the cached Zitadel machine key; re-run 'cargo xtask zitadel-bootstrap' from backend/ once Zitadel is up"
    fi
}

case "$command" in
    up)
        preflight
        docker_cmd "${compose_up[@]}" up -d ${extra[@]+"${extra[@]}"}
        ;;
    down)
        if [ "$volumes" = 1 ]; then
            confirm_volumes
            docker_cmd "${compose_all[@]}" down -v ${extra[@]+"${extra[@]}"}
            forget_machine_key
        else
            docker_cmd "${compose_all[@]}" down ${extra[@]+"${extra[@]}"}
        fi
        ;;
    restart | ps)
        docker_cmd "${compose_all[@]}" "$command" ${extra[@]+"${extra[@]}"}
        ;;
    logs)
        docker_cmd "${compose_all[@]}" logs -f ${extra[@]+"${extra[@]}"}
        ;;
    config)
        docker_cmd "${compose_up[@]}" config --no-env-resolution ${extra[@]+"${extra[@]}"}
        ;;
    build)
        docker_cmd "${compose_up[@]}" build ${extra[@]+"${extra[@]}"}
        ;;
esac
