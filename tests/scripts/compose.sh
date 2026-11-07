#!/usr/bin/env bash
# Tests compose.sh and compose.ps1 against the fake docker. Run: bash tests/scripts/compose.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

chain() {
    printf 'compose -f %s/docker/docker-compose.yml -f %s/docker/docker-compose.%s.yml' "$root" "$root" "$1"
}

# A complete vault for $1 and both dev certificate files, through the overrides.
ready() {
    write_vault "$work/vault" "$1"
    mkdir -p "$work/certs"
    : >"$work/certs/postit.local.crt"
    : >"$work/certs/postit-dev-ca.crt"
    export POSTIT_VAULT_DIR="$work/vault" POSTIT_CERT_DIR="$work/certs"
}

test_help_and_bare_call_print_the_layout() {
    twin compose help
    check "help exits 0" is "$code" 0
    check_layout compose
    twin compose
    check "a bare call exits 0" is "$code" 0
    check_layout compose
    twin compose --help
    check "--help exits 0" is "$code" 0
    check_layout compose
}

test_unknown_command_is_exit_1() {
    twin compose frobnicate
    check "exit 1" is "$code" 1
    err_has "unknown command: frobnicate"
}

test_up_builds_the_chain() {
    ready qa
    twin compose up qa
    check "exit 0" is "$code" 0
    check "one call, the whole chain (log: $log)" is "$log" "$(chain qa) up -d"
}

test_up_development_app_adds_the_profile() {
    ready development
    twin compose up --app
    check "exit 0 ($err)" is "$code" 0
    check "app profile (log: $log)" is "$log" "$(chain development) --profile app up -d"
}

test_app_outside_development_is_refused() {
    ready qa
    twin compose up qa --app
    check "exit 1" is "$code" 1
    check "nothing logged" is "$log" ""
}

test_up_without_vault_files_names_them() {
    ready qa
    rm -rf "$work/vault"
    mkdir -p "$work/vault"
    twin compose up qa
    check "exit 1" is "$code" 1
    err_has "!ref/vault/qa/postgres.env"
    err_has "!ref/vault/qa/postit.env"
    check "nothing logged" is "$log" ""
}

test_up_development_without_cert_is_refused() {
    ready development
    rm "$work/certs/postit.local.crt"
    twin compose up
    check "exit 1" is "$code" 1
    check "names the Bash cert script" has "$err" "./cert.sh"
    [ "$ps_ran" = 0 ] || check "names the PowerShell cert script" has "$ps_err" "./cert.ps1"
    check "nothing logged" is "$log" ""
}

test_up_development_app_without_ca_is_refused() {
    ready development
    rm "$work/certs/postit-dev-ca.crt"
    twin compose up --app
    check "exit 1" is "$code" 1
    err_has "postit-dev-ca.crt"
    check "nothing logged" is "$log" ""
}

test_config_never_resolves_env() {
    twin compose config qa
    check "exit 0" is "$code" 0
    check "no env resolution (log: $log)" is "$log" "$(chain qa) config --no-env-resolution"
}

test_ps_logs_restart_see_every_profile() {
    twin compose ps
    check "ps (log: $log)" is "$log" "$(chain development) --profile * ps"
    twin compose restart qa
    check "restart (log: $log)" is "$log" "$(chain qa) --profile * restart"
    twin compose logs development postit-zitadel --tail=10
    check "logs follows and passes the rest through (log: $log)" is "$log" "$(chain development) --profile * logs -f postit-zitadel --tail=10"
}

test_build_development() {
    twin compose build --app
    check "build (log: $log)" is "$log" "$(chain development) --profile app build"
}

test_compose_exit_code_passes_through() {
    export FAKE_DOCKER_FAIL=' ps$'
    twin compose ps
    check "docker's own exit code" is "$code" 1
}

# The cached Zitadel machine key, through the override, recreated before each twin run;
# $work/.kept lists the runs that left it in place.
machine_key() {
    export POSTIT_MACHINE_KEY="$work/machinekey/postit-bootstrap.json"
    twin_setup() { mkdir -p "$work/machinekey" && echo '{}' >"$POSTIT_MACHINE_KEY"; }
    twin_after() { [ ! -e "$POSTIT_MACHINE_KEY" ] || echo "$1" >>"$work/.kept"; }
    : >"$work/.kept"
}

kept() { tr -d '\n' <"$work/.kept"; }

test_down_volumes_needs_the_typed_environment() {
    machine_key
    printf 'development\n' | twin compose down --volumes
    check "exit 0 ($err)" is "$code" 0
    check "down -v last (log: $log)" is "${log##*$'\n'}" "$(chain development) --profile * down -v"
    check "machine key removed by every twin (kept by: $(kept))" is "$(kept)" ""
    printf 'nope\n' | twin compose down --volumes
    check "a wrong answer is exit 1" is "$code" 1
    check "a wrong answer runs no down" lacks "$log" " down"
    check "a wrong answer keeps the machine key" has "$(kept)" sh
}

test_down_volumes_aborts_on_empty_input() {
    machine_key
    twin compose down --volumes </dev/null
    check "exit 1" is "$code" 1
    check "no down" lacks "$log" " down"
    check "machine key kept" has "$(kept)" sh
}

test_down_without_volumes_asks_nothing() {
    twin compose down qa </dev/null
    check "exit 0" is "$code" 0
    check "plain down (log: $log)" is "$log" "$(chain qa) --profile * down"
}

test_down_volumes_force_skips_the_prompt_outside_production() {
    twin compose down qa --volumes --force </dev/null
    check "exit 0 ($err)" is "$code" 0
    check "down -v last (log: $log)" is "${log##*$'\n'}" "$(chain qa) --profile * down -v"
}

test_production_down_volumes_needs_force_and_the_prompt() {
    printf 'production\n' | twin compose down production --volumes
    check "without --force: exit 1" is "$code" 1
    check "without --force: nothing logged" is "$log" ""
    printf 'production\n' | twin compose down production --volumes --force
    check "with --force and the answer: exit 0 ($err)" is "$code" 0
    check "with --force and the answer: down -v (log: $log)" is "${log##*$'\n'}" "$(chain production) --profile * down -v"
    twin compose down production --volumes --force </dev/null
    check "with --force and no answer: exit 1" is "$code" 1
    check "with --force and no answer: no down" lacks "$log" " down"
}

# Every environment shares the project, the container names, and the database volume, so
# naming qa while production runs must not delete production's volume under qa's rules.
test_down_volumes_refuses_another_running_environment() {
    export FAKE_DOCKER_CONFIG_FILES="$root/docker/docker-compose.yml,$root/docker/docker-compose.production.yml"
    twin compose down qa --volumes --force </dev/null
    check "exit 1" is "$code" 1
    err_has "belongs to production"
    check "no down" lacks "$log" " down"
}

test_down_volumes_matching_running_environment_proceeds() {
    export FAKE_DOCKER_CONFIG_FILES="$root/docker/docker-compose.yml,$root/docker/docker-compose.qa.yml"
    twin compose down qa --volumes --force </dev/null
    check "exit 0 ($err)" is "$code" 0
    check "down -v last (log: $log)" is "${log##*$'\n'}" "$(chain qa) --profile * down -v"
}

test_confirm_without_a_trailing_newline() {
    printf 'development' | twin compose down --volumes
    check "exit 0 ($err $ps_err)" is "$code" 0
    check "down -v last (log: $log)" is "${log##*$'\n'}" "$(chain development) --profile * down -v"
}

test_environment_after_a_flag() {
    ready qa
    twin compose up --app qa
    check "--app with qa after it is refused" is "$code" 1
    err_has "development only"
    check "nothing logged" is "$log" ""
    twin compose config --app development
    check "exit 0" is "$code" 0
    check "development with the app profile (log: $log)" is "$log" "$(chain development) --profile app config --no-env-resolution"
}

test_compose_works_from_another_directory() {
    mkdir -p "$work/elsewhere"
    cd "$work/elsewhere" || return
    twin compose config qa
    check "exit 0" is "$code" 0
    check "the repository's files (log: $log)" is "$log" "$(chain qa) config --no-env-resolution"
}

test_compose_passes_windows_paths_under_msys() {
    command -v cygpath >/dev/null 2>&1 || return 0
    twin compose config qa
    check "something was logged" has "$log" docker-compose
    local word
    for word in $log; do
        case "$word" in *docker-compose*) check "Windows path form: $word" matches "$word" '^[A-Za-z]:/' ;; esac
    done
}

run_tests
