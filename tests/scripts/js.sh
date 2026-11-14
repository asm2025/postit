#!/usr/bin/env bash
# Tests js.sh and js.ps1 against a fake pnpm. Run: bash tests/scripts/js.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# A developer or CI run must not switch the default install to frozen-lockfile mode.
unset CI GITHUB_ACTIONS GITLAB_CI JENKINS_HOME TF_BUILD CIRCLECI

test_help_layout() {
    twin js help
    check "exit 0" is "$code" 0
    check_layout js
    twin js
    check "a bare call exits 0" is "$code" 0
    check_layout js
}

test_unknown_command_is_exit_1() {
    twin js frobnicate
    check "exit 1" is "$code" 1
    err_has "unknown command: frobnicate"
}

test_unknown_option_is_exit_1() {
    TWIN_FAKES=pnpm twin js install -x
    check "exit 1" is "$code" 1
    err_has "unknown option: -x"
    check "nothing logged" is "$log" ""
}

test_install_prefers_the_frozen_lockfile() {
    TWIN_FAKES=pnpm twin js install
    check "exit 0 ($err)" is "$code" 0
    log_in_order 'pnpm install --prefer-frozen-lockfile * in=web *'
}

test_install_c_freezes_the_lockfile() {
    TWIN_FAKES=pnpm twin js install -c
    check "exit 0 ($err)" is "$code" 0
    log_in_order 'pnpm install --frozen-lockfile * in=web *'
}

test_install_in_ci_freezes_the_lockfile() {
    export CI=true
    TWIN_FAKES=pnpm twin js install
    check "exit 0 ($err)" is "$code" 0
    log_in_order 'pnpm install --frozen-lockfile * in=web *'
}

test_update_stays_in_range_unless_asked() {
    TWIN_FAKES=pnpm twin js update
    log_in_order 'pnpm update db=* in=web *'
    check "no --latest" lacks "$log" "--latest"
    TWIN_FAKES=pnpm twin js update -l
    check "exit 0" is "$code" 0
    log_in_order 'pnpm update --latest * in=web *'
}

test_self_update_updates_pnpm_once() {
    TWIN_FAKES=pnpm twin js self-update
    check "exit 0 ($err)" is "$code" 0
    log_in_order 'pnpm self-update * in=postit *'
    check "called once" is "$(grep -c self-update <<<"$log")" 1
}

test_build_runs_the_build_script() {
    TWIN_FAKES=pnpm twin js build
    log_in_order 'pnpm run build * in=web *'
}

test_check_runs_the_gate_in_order() {
    TWIN_FAKES=pnpm twin js check
    check "exit 0" is "$code" 0
    log_in_order 'pnpm run typecheck *' 'pnpm run lint *' 'pnpm run format:check *' 'pnpm run test *' 'pnpm run build *'
}

test_check_stops_at_the_first_failure() {
    export FAKE_PNPM_FAIL=1
    TWIN_FAKES=pnpm twin js check
    check "exit 1" is "$code" 1
    check "stopped after typecheck" lacks "$log" "pnpm run lint"
}

test_audit_and_outdated() {
    TWIN_FAKES=pnpm twin js audit
    log_in_order 'pnpm audit *'
    TWIN_FAKES=pnpm twin js outdated
    check "exit 0" is "$code" 0
    log_in_order 'pnpm outdated *'
}

test_outdated_findings_are_not_a_failure() {
    export FAKE_PNPM_FAIL=1
    TWIN_FAKES=pnpm twin js outdated
    check "exit 0" is "$code" 0
}

test_list_names_the_web_project() {
    TWIN_FAKES=pnpm twin js list
    check "exit 0" is "$code" 0
    out_has "web"
    check "pnpm not called" is "$log" ""
}

test_missing_pnpm_is_exit_1() {
    export POSTIT_PNPM="$work/no-such-pnpm"
    twin js install
    check "exit 1" is "$code" 1
    err_has "pnpm not found"
}

run_tests
