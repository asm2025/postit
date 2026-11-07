#!/usr/bin/env bash
# Tests db.sh and db.ps1 against the fake docker. Run: bash tests/scripts/db.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

chain() {
    printf 'compose -f %s/docker/docker-compose.yml -f %s/docker/docker-compose.%s.yml' "$root" "$root" "$1"
}

# The running postit-postgres belongs to $1.
running() {
    export FAKE_DOCKER_CONFIG_FILES="$root/docker/docker-compose.yml,$root/docker/docker-compose.$1.yml"
}

test_help_layout() {
    twin db help
    check "exit 0" is "$code" 0
    check_layout db
    twin db
    check "a bare call exits 0" is "$code" 0
    check_layout db
}

test_unknown_command_is_exit_1() {
    twin db frobnicate development
    check "exit 1" is "$code" 1
    err_has "unknown command: frobnicate"
}

test_environment_is_required() {
    twin db backup
    check "exit 1" is "$code" 1
    err_has "<environment>"
    check "nothing logged" is "$log" ""
    twin db backup staging
    check "an unknown environment is exit 1" is "$code" 1
    err_has "<environment>"
}

test_psql_runs_in_the_container() {
    running development
    twin db psql development
    check "exit 0 ($err)" is "$code" 0
    log_in_order 'exec -it postit-postgres sh -c exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
}

test_psql_refuses_another_environment() {
    running development
    twin db psql qa
    check "exit 1" is "$code" 1
    err_has "belongs to development"
    check "no psql" lacks "$log" "exec -it"
}

test_backup_refuses_another_environment() {
    running qa
    twin db backup development
    check "exit 1" is "$code" 1
    err_has "belongs to qa"
    check "no pg_dump" lacks "$log" pg_dump
}

test_backup_refuses_without_a_container() {
    twin db backup development
    check "exit 1" is "$code" 1
    err_has "cannot tell which environment"
}

test_backup_refuses_an_unignored_repo_path() {
    running development
    export POSTIT_BACKUP_DIR="$root/docs/tmp-dumps-$$"
    twin db backup development
    check "exit 1" is "$code" 1
    err_has "git does not ignore"
    check "the directory was not created" test ! -e "$POSTIT_BACKUP_DIR"
    check "no pg_dump" lacks "$log" pg_dump
    rm -rf "$POSTIT_BACKUP_DIR"
}

test_backup_writes_and_cleans_up() {
    running development
    export POSTIT_BACKUP_DIR="$work/dumps"
    twin db backup development
    check "exit 0 ($err)" is "$code" 0
    log_in_order \
        'exec postit-postgres sh -c pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump' \
        "cp postit-postgres:/tmp/postit.dump $work/dumps/postit-development-*.dump" \
        'exec postit-postgres sh -c rm -f /tmp/postit.dump'
    check "reports the dump's name ($out)" matches "$out" 'postit-development-[0-9]{8}T[0-9]{6}Z\.dump'
    check "the dump exists" eval "compgen -G \"$work/dumps/postit-development-*.dump\" >/dev/null"
}

test_backup_failure_still_cleans_the_container() {
    running development
    export POSTIT_BACKUP_DIR="$work/dumps" FAKE_DOCKER_FAIL='^cp '
    twin db backup development
    check "exit 1" is "$code" 1
    err_has "docker cp failed"
    log_in_order 'exec postit-postgres sh -c rm -f /tmp/postit.dump'
}

# A dump file named for $1.
dump() {
    dumpfile="$work/postit-$1-20260101T000000Z.dump"
    : >"$dumpfile"
}

test_restore_refuses_a_dump_named_for_another_environment() {
    running development
    dump qa
    twin db restore development "$dumpfile"
    check "exit 1" is "$code" 1
    err_has "is not a postit-development-*.dump file"
    check "nothing logged" is "$log" ""
}

test_restore_needs_a_file() {
    running development
    twin db restore development
    check "exit 1" is "$code" 1
    err_has "<file>"
    twin db restore development "$work/nope.dump"
    check "a missing file is exit 1" is "$code" 1
    err_has "no such file"
}

test_restore_refuses_while_the_server_runs() {
    running development
    dump development
    export FAKE_DOCKER_PS=postit-api
    twin db restore development "$dumpfile"
    check "exit 1" is "$code" 1
    err_has "docker stop postit-api"
    check "no pg_restore" lacks "$log" pg_restore
}

test_restore_development() {
    running development
    dump development
    twin db restore development "$dumpfile"
    check "exit 0 ($err)" is "$code" 0
    log_in_order \
        "cp $dumpfile postit-postgres:/tmp/postit.dump" \
        'exec postit-postgres sh -c pg_restore --single-transaction --exit-on-error --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump' \
        'exec postit-postgres sh -c rm -f /tmp/postit.dump'
}

test_production_restore_needs_force() {
    running production
    dump production
    twin db restore production "$dumpfile"
    check "without --force: exit 1" is "$code" 1
    check "without --force: no pg_restore" lacks "$log" pg_restore
    twin db restore production "$dumpfile" --force
    check "with --force: exit 0 ($err)" is "$code" 0
    log_in_order '*pg_restore --single-transaction*'
}

test_drop_qa_needs_no_stop() {
    twin db drop qa --force
    check "exit 1" is "$code" 1
    err_has "docker stop postit-api postit-worker"
    check "nothing logged" is "$log" ""
}

test_drop_production_refuses_force() {
    twin db drop production --force --no-stop
    check "exit 1" is "$code" 1
    check "nothing logged" is "$log" ""
}

test_drop_development_stops_drops_recreates_restarts() {
    printf 'postit\n' | twin db drop development
    check "exit 0 ($err)" is "$code" 0
    log_in_order \
        "$(chain development) --profile * stop postit-server" \
        '*DROP DATABASE IF EXISTS "postit" WITH (FORCE);' \
        '*CREATE DATABASE "postit";' \
        "$(chain development) --profile * start postit-server"
    check "the zitadel database is never named" lacks "$log" zitadel
}

test_drop_development_restarts_only_what_ran() {
    export FAKE_DOCKER_SERVICES='postit-postgres postit-zitadel'
    printf 'postit\n' | twin db drop development
    check "exit 0 ($err)" is "$code" 0
    check "nothing stopped" lacks "$log" " stop "
    check "nothing started" lacks "$log" " start "
    log_in_order '*DROP DATABASE IF EXISTS "postit" WITH (FORCE);'
}

test_drop_aborts_on_empty_input() {
    twin db drop development </dev/null
    check "exit 1" is "$code" 1
    check "no DROP" lacks "$log" DROP
    printf 'nope\n' | twin db drop development
    check "a wrong answer is exit 1" is "$code" 1
    check "a wrong answer drops nothing" lacks "$log" DROP
}

test_drop_aborted_stops_nothing() {
    twin db drop development </dev/null
    check "nothing stopped (log: $log)" lacks "$log" " stop "
}

test_drop_qa_no_stop_force_drops() {
    twin db drop qa --no-stop --force </dev/null
    check "exit 0 ($err)" is "$code" 0
    log_in_order '*DROP DATABASE IF EXISTS "postit" WITH (FORCE);' '*CREATE DATABASE "postit";'
    check "nothing stopped" lacks "$log" " stop "
}

test_drop_needs_a_running_postgres() {
    export FAKE_DOCKER_RUNNING=false
    twin db drop qa --no-stop --force
    check "exit 1" is "$code" 1
    err_has "postit-postgres is not running"
    check "no DROP" lacks "$log" DROP
}

run_tests
