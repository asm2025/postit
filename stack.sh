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
#   doctor      Check the environment's vault files and required values.  No Docker, no
#               network, and no value is ever printed
#   build       Build the postit-server image (development: add --app)
#   psql        Open a psql shell on the postit database, as the vault's user
#   backup      Dump the postit database to !ref/backups/postit-<environment>-<UTC>.dump
#               (POSTIT_BACKUP_DIR overrides; inside this repository it must be git-ignored,
#               outside it any path is accepted).  Both backup and restore act on the
#               running postit-postgres and refuse when it belongs to another environment
#   restore     Restore a dump: ./stack.sh restore [environment] <file>.  One transaction:
#               a failure leaves the database as it was.  Refuses while the server runs;
#               production needs --force.  Does not start anything afterwards
#   reset       down -v, then up.  DESTROYS the database
#   help        Show this help message
#
# Environments:
#   development (default), qa, production
#
# Options:
#   --app          development: also run the `app` profile (postit-server behind
#                  postit-nginx-app on 44310/44311/44315).  Leave it off while `cargo run` / the web dev server
#                  own those ports
#   -v, --volumes  down: also destroy the database volume
#   --force        Required to destroy the production database (down -v, reset)
#
# Examples:
#   ./stack.sh up                        Development infrastructure, the default
#   ./stack.sh up --app                  ...plus postit-server behind postit-nginx-app
#   ./stack.sh logs development postit-zitadel
#   ./stack.sh down -v                   Development, database destroyed
#   ./stack.sh config qa                 What qa resolves to, vault files by path
#   ./stack.sh reset production --force

set -euo pipefail

# `pwd -W` (Git Bash / MSYS only) yields D:/... instead of /d/..., which a Windows
# docker.exe would otherwise read as a path on the current drive (D:\d\...).
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && { pwd -W 2>/dev/null || pwd; })"
docker_dir="$script_dir/docker"
# Overridable for tests (docker/tests/stack-doctor.sh) and unusual layouts.
vault_dir="${POSTIT_VAULT_DIR:-$script_dir/!ref/vault}"
docker_env_file="${POSTIT_DOCKER_ENV_FILE:-$docker_dir/.env}"
backup_dir="${POSTIT_BACKUP_DIR:-$script_dir/!ref/backups}"
cert_file="$docker_dir/shared/nginx/certs/postit.local.crt"
ca_file="$docker_dir/shared/nginx/certs/postit-dev-ca.crt"

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
    if [ "$environment" = "development" ] && [ "$app" = 1 ] && [ ! -f "$ca_file" ]; then
        die "no dev CA at docker/shared/nginx/certs/postit-dev-ca.crt — run ./cert.sh first"
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

# KEY's value in a dotenv file, read as data (a vault file is never sourced). Surrounding
# single or double quotes are stripped, matching postit-config and Compose.
env_value() {
    local file="$1" key="$2" line value
    [ -f "$file" ] || return 0
    line="$(grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 || true)"
    [ -n "$line" ] || return 0
    value="${line#*=}"
    value="${value%$'\r'}"
    case "$value" in
        \'*\') value="${value:1:${#value}-2}" ;;
        \"*\") value="${value:1:${#value}-2}" ;;
    esac
    printf '%s' "$value"
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
production_compose_vars="POSTIT_API_URL POSTIT_APP_URL POSTIT_OIDC_ISSUER POSTIT_OIDC_AUDIENCE POSTIT_SMTP_HOST POSTIT_MAIL_FROM"

doctor() {
    local problems=() f key dir="$vault_dir/$environment"
    for f in $(vault_files); do
        [ -f "$dir/$f" ] || problems+=("missing !ref/vault/$environment/$f")
    done
    if [ ${#problems[@]} -eq 0 ]; then
        local pg="$dir/postgres.env" app="$dir/postit.env"
        for key in POSTGRES_USER POSTGRES_PASSWORD; do
            [ -n "$(env_value "$pg" "$key")" ] || problems+=("postgres.env: $key is missing or empty")
        done
        for key in POSTIT__DATABASE__USERNAME POSTIT__DATABASE__PASSWORD POSTIT__AUDIT__PSEUDONYM_KEY; do
            [ -n "$(env_value "$app" "$key")" ] || problems+=("postit.env: $key is missing or empty")
        done
        if [ "$environment" = "development" ] && [ -z "$(env_value "$dir/zitadel.env" ZITADEL_MASTERKEY)" ]; then
            problems+=("zitadel.env: ZITADEL_MASTERKEY is missing or empty")
        fi
        if [ "$environment" = "development" ] && [ -z "$(env_value "$app" POSTIT__IDENTITY_MAIL__SIGNING_KEY)" ]; then
            problems+=("postit.env: POSTIT__IDENTITY_MAIL__SIGNING_KEY is missing or empty: run cargo xtask zitadel-bootstrap")
        fi
        local pkey
        pkey="$(env_value "$app" POSTIT__AUDIT__PSEUDONYM_KEY)"
        if [ -n "$pkey" ] && [ ${#pkey} -lt 32 ]; then
            problems+=("postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters")
        fi
        if [ "$environment" != "development" ]; then
            local vk vkeys=() vname
            while IFS= read -r vname; do vkeys+=("$vname"); done < <(
                sed -nE 's/^[[:space:]]*(POSTIT__VAULT__KEYS__[1-9][0-9]*)=.*/\1/p' "$app" | sort -u)
            if [ ${#vkeys[@]} -eq 0 ]; then
                problems+=("postit.env: POSTIT__VAULT__KEYS__1 is missing or empty")
            fi
            for vname in ${vkeys[@]+"${vkeys[@]}"}; do
                vk="$(env_value "$app" "$vname")"
                if [ -z "$vk" ]; then
                    problems+=("postit.env: $vname is missing or empty")
                elif [ ${#vk} -lt 32 ]; then
                    problems+=("postit.env: $vname is shorter than 32 characters")
                fi
            done
            for key in POSTIT__MEDIA__SIGNING_KEY POSTIT__EMAIL__SIGNING_KEY; do
                vk="$(env_value "$app" "$key")"
                if [ -z "$vk" ]; then
                    problems+=("postit.env: $key is missing or empty")
                elif [ ${#vk} -lt 32 ]; then
                    problems+=("postit.env: $key is shorter than 32 characters")
                fi
            done
        fi
        [ "$(env_value "$pg" POSTGRES_USER)" = "$(env_value "$app" POSTIT__DATABASE__USERNAME)" ] ||
            problems+=("postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env")
        [ "$(env_value "$pg" POSTGRES_PASSWORD)" = "$(env_value "$app" POSTIT__DATABASE__PASSWORD)" ] ||
            problems+=("postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env")
        local smtp_user smtp_pass
        smtp_user="$(env_value "$app" POSTIT__MAIL__SMTP__USERNAME)"
        smtp_pass="$(env_value "$app" POSTIT__MAIL__SMTP__PASSWORD)"
        if [ -n "$smtp_user" ] && [ -z "$smtp_pass" ]; then
            problems+=("postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not")
        elif [ -z "$smtp_user" ] && [ -n "$smtp_pass" ]; then
            problems+=("postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not")
        fi
    fi
    if [ "$environment" = "production" ]; then
        for key in $production_compose_vars; do
            [ -n "${!key:-}" ] || [ -n "$(env_value "$docker_env_file" "$key")" ] ||
                problems+=("$key is not set in the shell or docker/.env (see docker/.env.example)")
        done
    fi
    if [ ${#problems[@]} -gt 0 ]; then
        echo "stack: $environment is not ready:" >&2
        printf '  %s\n' "${problems[@]}" >&2
        exit 1
    fi
    echo "stack: $environment looks complete"
}

# Absolute, Windows-style under Git Bash (git, mkdir and docker cp must all see the same
# path); the directory need not exist yet.
abs_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -m -a "$1"
    elif command -v realpath >/dev/null 2>&1; then
        realpath -m "$1"
    else
        case "$1" in /*) printf '%s' "$1" ;; *) printf '%s/%s' "$PWD" "$1" ;; esac
    fi
}

# The dump is unencrypted user data: never leave it in the container's /tmp. The command
# runs inside sh -c so Git Bash cannot rewrite /tmp/... into a Windows path.
drop_container_dump() {
    docker exec postit-postgres sh -c 'rm -f /tmp/postit.dump' >/dev/null 2>&1 || true
}

# The environment the running postit-postgres was started for, read from the compose
# config-file label (".../docker-compose.<environment>.yml"). Empty when there is no
# container or no label. Backup and restore act on that container whatever environment
# was typed, so they check the two agree first.
running_environment() {
    local files
    files="$(docker inspect postit-postgres \
        --format '{{index .Config.Labels "com.docker.compose.project.config_files"}}' 2>/dev/null || true)"
    printf '%s' "$files" | tr ',' '\n' |
        sed -nE 's#.*docker-compose\.(development|qa|production)\.yml\r?$#\1#p' | tail -n 1
}

# Sets $running_env, or dies when it cannot be determined or differs from $environment.
require_running_environment() {
    running_env="$(running_environment)"
    [ -n "$running_env" ] ||
        die "cannot tell which environment postit-postgres belongs to (no such container, or it was not started by this launcher); refusing to touch it"
    [ "$running_env" = "$environment" ] ||
        die "you asked for $environment, but the running postit-postgres belongs to $running_env; refusing"
}

backup() {
    local stamp dir file rc=0
    require_running_environment
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    dir="$(abs_path "$backup_dir")"
    file="$dir/postit-$environment-$stamp.dump"
    # Checked before anything is created: a refused destination gets no new directory.
    # check-ignore exits 0 (ignored), 1 (inside the work tree, not ignored) or 128 (outside
    # any work tree, or no git); only the inside case needs the rail.
    git -C "$script_dir" check-ignore -q "$file" 2>/dev/null || rc=$?
    case "$rc" in
        0) ;;
        1) die "refusing to write a dump to $file: it is inside the repository and git does not ignore that path, and a dump is unencrypted user data" ;;
        *) echo "stack: $dir is outside the repository's git work tree, so nothing guards it from being committed; the directory is owner-only" ;;
    esac
    (umask 077; mkdir -p "$dir")
    chmod 700 "$dir" 2>/dev/null || true
    trap drop_container_dump EXIT
    # Written inside the container and copied out: binary-safe on every host shell.
    docker exec postit-postgres sh -c 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump' ||
        die "pg_dump failed"
    docker cp postit-postgres:/tmp/postit.dump "$file" || {
        rm -f "$file"
        die "docker cp failed; no dump was kept"
    }
    chmod 600 "$file" 2>/dev/null || true
    echo "stack: wrote $file"
    echo "stack: it holds every user's data unencrypted; delete it when you are done with it"
}

restore() {
    local file="${extra[0]:-}" running
    [ -n "$file" ] || die "usage: ./stack.sh restore [environment] <dump file>"
    [ -f "$file" ] || die "no such file: $file"
    case "$(basename "$file")" in
        "postit-$environment-"*.dump) ;;
        *) die "refusing: $(basename "$file") is not a postit-$environment-*.dump file" ;;
    esac
    require_running_environment
    # The guard follows the database that will be overwritten, not the word that was typed.
    if [ "$running_env" = "production" ] && [ "$force" = 0 ]; then
        die "refusing to overwrite the production database without --force"
    fi
    running="$(docker ps --format '{{.Names}}' | grep -E '^postit-(api|worker|server)$' || true)"
    [ -z "$running" ] ||
        die "stop the server first (docker stop $(echo $running)): it would keep writing to a half-restored database"
    trap drop_container_dump EXIT
    docker cp "$file" postit-postgres:/tmp/postit.dump || die "docker cp failed; nothing was restored"
    docker exec postit-postgres sh -c \
        'pg_restore --single-transaction --exit-on-error --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump' ||
        die "pg_restore failed; it ran as one transaction, so the database is as it was before"
    echo "stack: restored $file; start the server with ./stack.sh up $environment"
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
        echo "stack: removed the cached Zitadel machine key; re-run 'cargo xtask zitadel-bootstrap' from backend/ once Zitadel is up"
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
        "${compose_up[@]}" build ${extra[@]+"${extra[@]}"}
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
    doctor)
        doctor
        ;;
    backup)
        backup
        ;;
    restore)
        restore
        ;;
    help|-h|--help)
        usage
        ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac
