#!/usr/bin/env bash
# The postit database: psql, backup, restore, drop, and the sqlx cache. Twin: db.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=db
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
db - work with the postit database of one environment.

USAGE
  ./db.sh <command> <environment> [options]
  ./db.sh restore <environment> <file> [--force]

COMMANDS
  psql      Open psql inside postit-postgres, as the container's own user over the
            local socket (no password is handled)
  backup    Dump the database to !ref/backups/postit-<environment>-<UTC>.dump
  restore   Restore a dump in one transaction; a failure leaves the database as it
            was. Refuses while the server runs; starts nothing afterwards
  drop      Drop the postit database and recreate it empty; the server recreates
            the schema on its next boot. The zitadel database is never touched
  help      Show this help (also -h, --help, or no command)

  psql, backup, and restore act on the running postit-postgres and refuse when it
  belongs to another environment than the one you name.

OPTIONS
  environment   development, qa, or production (required)
  --force       restore: required to overwrite the production database.
                drop: skip the typed confirmation in development and qa;
                production always asks and refuses --force
  --no-stop     drop: you stopped postit-api and postit-worker yourself. Required
                in qa and production; in development the script stops and restarts
                the running app services itself

  POSTIT_BACKUP_DIR    backup destination; inside this repository it must be a
                       git-ignored path (a dump is unencrypted user data)
  ADMIN_DATABASE_URL   drop: postgres://user:password@host:port/db to reach a
                       database elsewhere through a host psql

EXAMPLES
  ./db.sh psql development
  ./db.sh backup development
  ./db.sh restore development '!ref/backups/postit-development-20261107T120000Z.dump'
  ./db.sh drop development
  ./db.sh drop qa --no-stop

EXIT CODES
  0  success
  1  usage error, a refused guard, an unconfirmed question, or a failure

MORE
  docs/scripts.md                       every operator script and the recipes
  docs/deployment.md                    backups and restores outside development
EOF
}

command="${1:-help}"
[ $# -gt 0 ] && shift

case "$command" in
    help | -h | --help)
        usage
        exit 0
        ;;
    psql | backup | restore | drop) ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac

environment="$(parse_environment "${1:-}")"
if [ -z "$environment" ]; then
    case "$command" in
        restore) die "usage: ./db.sh restore <environment> <file> [--force]  (<environment> is development, qa, or production)" ;;
        *) die "usage: ./db.sh $command <environment>  (<environment> is development, qa, or production)" ;;
    esac
fi
shift

force=0
no_stop=0
positional=()
for arg in "$@"; do
    case "$arg" in
        --force) force=1 ;;
        --no-stop) no_stop=1 ;;
        -*) die "unknown option: $arg" ;;
        *) positional+=("$arg") ;;
    esac
done

mapfile -t compose < <(compose_args "$environment")
compose_all=("${compose[@]}" --profile '*')

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
    docker_cmd exec postit-postgres sh -c 'rm -f /tmp/postit.dump' >/dev/null 2>&1 || true
}

# The environment the running postit-postgres was started for, read from the compose
# config-file label (".../docker-compose.<environment>.yml"). Empty when there is no
# container or no label.
running_environment() {
    local files
    files="$(docker_cmd inspect postit-postgres \
        --format '{{index .Config.Labels "com.docker.compose.project.config_files"}}' 2>/dev/null || true)"
    printf '%s' "$files" | tr ',' '\n' |
        sed -nE 's#.*docker-compose\.(development|qa|production)\.yml\r?$#\1#p' | tail -n 1
}

# psql, backup and restore act on that container whatever environment was typed, so
# they check the two agree first.
require_running_environment() {
    local running_env
    running_env="$(running_environment)"
    [ -n "$running_env" ] ||
        die "cannot tell which environment postit-postgres belongs to (no such container, or it was not started by ./compose.sh); refusing to touch it"
    [ "$running_env" = "$environment" ] ||
        die "you asked for $environment, but the running postit-postgres belongs to $running_env; refusing"
}

do_psql() {
    require_running_environment
    # The container's own POSTGRES_USER (from the vault) over the local socket, which the
    # postgres image trusts: no password on the command line or in the shell.
    docker_cmd exec -it postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
}

do_backup() {
    local stamp dir file rc=0
    require_running_environment
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    dir="$(abs_path "${POSTIT_BACKUP_DIR:-$POSTIT_ROOT/!ref/backups}")"
    file="$dir/postit-$environment-$stamp.dump"
    # Checked before anything is created: a refused destination gets no new directory.
    # check-ignore exits 0 (ignored), 1 (inside the work tree, not ignored) or 128 (outside
    # any work tree, or no git); only the inside case needs the rail.
    git -C "$POSTIT_ROOT" check-ignore -q "$file" 2>/dev/null || rc=$?
    case "$rc" in
        0) ;;
        1) die "refusing to write a dump to $file: it is inside the repository and git does not ignore that path, and a dump is unencrypted user data" ;;
        *) say "$dir is outside the repository's git work tree, so nothing guards it from being committed; the directory is owner-only" ;;
    esac
    (umask 077 && mkdir -p "$dir")
    chmod 700 "$dir" 2>/dev/null || true
    trap drop_container_dump EXIT
    # Written inside the container and copied out: binary-safe on every host shell.
    docker_cmd exec postit-postgres sh -c 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump' ||
        die "pg_dump failed"
    docker_cmd cp postit-postgres:/tmp/postit.dump "$file" || {
        rm -f "$file"
        die "docker cp failed; no dump was kept"
    }
    chmod 600 "$file" 2>/dev/null || true
    say "wrote $file"
    say "it holds every user's data unencrypted; delete it when you are done with it"
}

do_restore() {
    local file="${positional[0]:-}" running
    [ -n "$file" ] || die "usage: ./db.sh restore <environment> <file> [--force]  (<file> is a postit-$environment-*.dump)"
    [ -f "$file" ] || die "no such file: $file"
    case "$(basename "$file")" in
        "postit-$environment-"*.dump) ;;
        *) die "refusing: $(basename "$file") is not a postit-$environment-*.dump file" ;;
    esac
    require_running_environment
    if [ "$environment" = production ] && [ "$force" = 0 ]; then
        die "refusing to overwrite the production database without --force"
    fi
    running="$(docker_cmd ps --format '{{.Names}}' | grep -E '^postit-(api|worker|server)$' || true)"
    [ -z "$running" ] ||
        die "stop the server first (docker stop $(echo $running)): it would keep writing to a half-restored database"
    trap drop_container_dump EXIT
    docker_cmd cp "$file" postit-postgres:/tmp/postit.dump || die "docker cp failed; nothing was restored"
    docker_cmd exec postit-postgres sh -c \
        'pg_restore --single-transaction --exit-on-error --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump' ||
        die "pg_restore failed; it ran as one transaction, so the database is as it was before"
    say "restored $file; start the server with ./compose.sh up $environment"
}

# The app services that connect to Postgres. postit-nginx-app never does, so it stays up.
migrating_services="postit-server postit-api postit-worker"

do_drop() {
    if [ "$environment" = production ] && [ "$force" = 1 ]; then
        die "production cannot be forced"
    fi
    if [ "$environment" != development ] && [ "$no_stop" = 0 ]; then
        die "$environment requires --no-stop: stop postit-api and postit-worker yourself first (docker stop postit-api postit-worker), then re-run with --no-stop"
    fi

    local db_name target maintenance_url=""
    if [ -n "${ADMIN_DATABASE_URL:-}" ]; then
        if [[ "$ADMIN_DATABASE_URL" =~ ^postgres(ql)?://([^:@/]+):([^@]*)@([^:/]+):([0-9]+)/([^?]+) ]]; then
            export PGPASSWORD="${BASH_REMATCH[3]}"
            target="${BASH_REMATCH[4]}:${BASH_REMATCH[5]}"
            db_name="${BASH_REMATCH[6]}"
            maintenance_url="postgres://${BASH_REMATCH[2]}@$target/postgres"
        else
            die "could not parse ADMIN_DATABASE_URL (expected postgres://user:password@host:port/db)"
        fi
    else
        # The database name is config (database.url in backend/config/<environment>.toml),
        # the same in every environment.
        db_name=postit
        target="the postit-postgres container"
        [ "$(docker_cmd inspect --format '{{.State.Running}}' postit-postgres 2>/dev/null || true)" = true ] ||
            die "postit-postgres is not running on this host; start it (./compose.sh up $environment), or set ADMIN_DATABASE_URL to reach a database elsewhere"
    fi

    # Asked before anything is stopped, so a "no" leaves everything as it was.
    if [ "$environment" = production ] || [ "$force" = 0 ]; then
        [ "$environment" != development ] || [ "$no_stop" = 1 ] ||
            echo "If a native 'cargo run' server is using this database, stop it first."
        confirm "$db_name" "Type the database name ($db_name) to drop it on $target: " ||
            die "confirmation did not match; nothing was dropped"
    fi

    # Development: stop the running app services, so nothing migrates mid-drop, and start
    # the same ones again afterwards.
    local stopped=() s running_now
    if [ "$environment" = development ] && [ "$no_stop" = 0 ]; then
        running_now="$(docker_cmd "${compose_all[@]}" ps --services --filter status=running 2>/dev/null || true)"
        for s in $migrating_services; do
            ! printf '%s\n' "$running_now" | grep -qx "$s" || stopped+=("$s")
        done
        if [ ${#stopped[@]} -gt 0 ]; then
            say "stopping ${stopped[*]}"
            docker_cmd "${compose_all[@]}" stop "${stopped[@]}"
        fi
    fi

    say "dropping database $db_name on $target"
    run_sql "$maintenance_url" "DROP DATABASE IF EXISTS \"$db_name\" WITH (FORCE);"
    # No OWNER clause: the connecting role owns it, as it owned the one just dropped.
    run_sql "$maintenance_url" "CREATE DATABASE \"$db_name\";"
    say "database $db_name dropped and recreated empty"

    if [ ${#stopped[@]} -gt 0 ]; then
        say "starting ${stopped[*]}"
        docker_cmd "${compose_all[@]}" start "${stopped[@]}"
        local all_up=0 _
        for _ in $(seq 1 30); do
            running_now="$(docker_cmd "${compose_all[@]}" ps --services --filter status=running 2>/dev/null || true)"
            all_up=1
            for s in "${stopped[@]}"; do
                printf '%s\n' "$running_now" | grep -qx "$s" || all_up=0
            done
            [ "$all_up" = 0 ] || break
            sleep 2
        done
        [ "$all_up" = 1 ] || echo "db: not every app service reported running after 60 s; check ./compose.sh ps" >&2
    fi
}

# run_sql <maintenance url or empty> <statement>: one statement against the maintenance
# database. In the container the statement is a positional argument, so no shell
# re-parses its quotes.
run_sql() {
    if [ -n "$1" ]; then
        psql "$1" -v ON_ERROR_STOP=1 -c "$2"
    else
        docker_cmd exec postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d postgres -v ON_ERROR_STOP=1 -c "$1"' sh "$2"
    fi
}

case "$command" in
    psql) do_psql ;;
    backup) do_backup ;;
    restore) do_restore ;;
    drop) do_drop ;;
esac
