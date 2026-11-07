#!/usr/bin/env bash
# Shared harness for the operator script tests (tests/scripts/*.sh). Sourced, never run.
#
# Each test is a function named test_<name>, run in its own subshell with a fresh $work
# directory. `twin <script> <args...>` runs the Bash half and, when pwsh is on PATH, the
# PowerShell half of a root script against fake docker executables, then checks that both
# exit alike and call docker alike. Needs Bash 4+; no Docker, no network, no real vault.

set -uo pipefail
# A piped `twin` runs twin in this shell, so $out and $code survive.
shopt -s lastpipe

# Windows form (D:/...) under Git Bash, so pwsh and docker.exe read the same paths.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && { pwd -W 2>/dev/null || pwd; })"
fakes="$root/tests/scripts/fakes"

winpath() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

have_pwsh() { [ -z "${POSTIT_TEST_NO_PWSH:-}" ] && command -v pwsh >/dev/null 2>&1; }

# --- assertions ---

_test=""
_test_failed=0

# check <why> <command...>: records a failure of the current test when the command fails.
check() {
    local why="$1"
    shift
    if ! "$@"; then
        _test_failed=1
        echo "    $_test: $why" >&2
    fi
}

has() { [[ "$1" == *"$2"* ]]; }
lacks() { [[ "$1" != *"$2"* ]]; }
matches() { [[ "$1" =~ $2 ]]; }
is() { [ "$1" = "$2" ]; }

# Both twins' stderr contain the text (the PowerShell one only when pwsh ran).
err_has() {
    check "stderr names '$1' (sh: $err)" has "$err" "$1"
    if [ "$ps_ran" = 1 ]; then check "PowerShell stderr names '$1' (ps: $ps_err)" has "$ps_err" "$1"; fi
}

out_has() {
    check "stdout names '$1' (sh: $out)" has "$out" "$1"
    if [ "$ps_ran" = 1 ]; then check "PowerShell stdout names '$1' (ps: $ps_out)" has "$ps_out" "$1"; fi
}

# The help layout every script shares.
check_layout() {
    local name="$1" section text
    for text in "$out" "${ps_out:-$out}"; do
        check "help starts with '$name - ' (got: ${text%%$'\n'*})" has "${text%%$'\n'*}" "$name - "
        for section in USAGE COMMANDS OPTIONS EXAMPLES "EXIT CODES" MORE; do
            check "help has the $section section" has "$text" "$section"
        done
    done
}

# The logged calls, one per line, contain these lines in this order (others may sit between).
# Each argument is a glob pattern for one whole line.
log_in_order() {
    local -a lines
    local want i=0
    mapfile -t lines <<<"$log"
    for want in "$@"; do
        # shellcheck disable=SC2053 # $want is a glob on purpose
        while [ "$i" -lt "${#lines[@]}" ] && [[ "${lines[$i]}" != $want ]]; do i=$((i + 1)); done
        if [ "$i" -ge "${#lines[@]}" ]; then
            _test_failed=1
            echo "    $_test: no logged call matching '$want' in order; log was:" >&2
            printf '%s\n' "$log" | sed 's/^/      /' >&2
            return 0
        fi
        i=$((i + 1))
    done
}

# --- running the twins ---

# Called before each twin run, so a fixture a run consumes (a file it deletes) is fresh
# for the other. Tests redefine it.
twin_setup() { :; }
# Called after each twin run with `sh` or `ps`, to look at what that run left behind.
twin_after() { :; }

# Bash flag spellings to their PowerShell ones.
_ps_args() {
    local a
    _pargs=()
    for a in "$@"; do
        case "$a" in
            --force) _pargs+=(-Force) ;;
            --volumes | -v) _pargs+=(-Volumes) ;;
            --app) _pargs+=(-App) ;;
            --no-stop) _pargs+=(-NoStop) ;;
            --new-password) _pargs+=(-NewPassword) ;;
            *) _pargs+=("$a") ;;
        esac
    done
}

# The fake docker, sqlx and cargo (tests/scripts/fakes) stand in for the real ones; a test
# that sets POSTIT_SQLX or POSTIT_CARGO itself keeps its value. TWIN_FAKES="7z" also points
# POSTIT_7Z at fakes/7z (Bash) and fakes/7z.ps1 (PowerShell); TWIN_FAKES="psql" or "curl" likewise POSTIT_PSQL or POSTIT_CURL. TWIN_DOCKER replaces the fake
# docker for both halves (a missing path stands for "Docker is not installed").
# twin <script> <args...>: sets $out $err $code $log (Bash run) and $ps_out $ps_err
# $ps_code $ps_log (PowerShell run; ps_ran=1 when it ran). Standard input is read once and
# handed to both runs; a terminal counts as empty input.
twin() {
    local script="$1"
    shift
    local stdin="$work/.stdin"
    if [ -t 0 ]; then : >"$stdin"; else cat >"$stdin"; fi

    : >"$work/.sh.log"
    twin_setup
    code=0
    (
        # A stock Git Bash converts POSIX-looking arguments for native programs; run like one.
        unset MSYS_NO_PATHCONV MSYS2_ARG_CONV_EXCL
        export POSTIT_DOCKER="${TWIN_DOCKER:-$fakes/docker}" POSTIT_FAKE_DOCKER_LOG="$work/.sh.log"
        export POSTIT_SQLX="${POSTIT_SQLX:-$fakes/sqlx}" POSTIT_CARGO="${POSTIT_CARGO:-$fakes/cargo}"
        for fake in ${TWIN_FAKES:-}; do export "POSTIT_${fake^^}=$fakes/$fake"; done
        exec bash "$root/$script.sh" "$@"
    ) <"$stdin" >"$work/.sh.out" 2>"$work/.sh.err" || code=$?
    out="$(tr -d '\r' <"$work/.sh.out")"
    err="$(tr -d '\r' <"$work/.sh.err")"
    log="$(tr -d '\r' <"$work/.sh.log")"
    twin_after sh

    ps_ran=0
    ps_out="" ps_err="" ps_log="" ps_code=""
    if have_pwsh; then
        ps_ran=1
        _ps_args "$@"
        : >"$work/.ps.log"
        twin_setup
        ps_code=0
        (
            unset MSYS_NO_PATHCONV MSYS2_ARG_CONV_EXCL
            export POSTIT_DOCKER="${TWIN_DOCKER:-$(winpath "$fakes/docker.ps1")}" POSTIT_FAKE_DOCKER_LOG="$(winpath "$work/.ps.log")"
            export POSTIT_SQLX="${POSTIT_SQLX:-$(winpath "$fakes/sqlx.ps1")}" POSTIT_CARGO="${POSTIT_CARGO:-$(winpath "$fakes/cargo.ps1")}"
            for fake in ${TWIN_FAKES:-}; do export "POSTIT_${fake^^}=$(winpath "$fakes/$fake.ps1")"; done
            exec pwsh -NoLogo -NoProfile -NonInteractive -File "$(winpath "$root/$script.ps1")" ${_pargs[@]+"${_pargs[@]}"}
        ) <"$stdin" >"$work/.ps.out" 2>"$work/.ps.err" || ps_code=$?
        ps_out="$(tr -d '\r' <"$work/.ps.out")"
        ps_err="$(tr -d '\r' <"$work/.ps.err")"
        ps_log="$(tr -d '\r' <"$work/.ps.log")"
        twin_after ps
        check "twins exit alike (sh $code, ps $ps_code; ps stderr: $ps_err)" is "$code" "$ps_code"
        # The two runs start seconds apart, so UTC stamps (20260101T000000Z) may differ, and
        # each has its own process ID or random part in temporary names
        # (vault.7z.tmp.<pid>.7z, postit-vault-check.<random>).
        local stamp='s/[0-9]{8}T[0-9]{6}Z/<stamp>/g; s/\.tmp\.[0-9]+\./.tmp.<pid>./g; s/postit-vault-check\.[A-Za-z0-9]+/postit-vault-check.<random>/g' a b
        a="$(printf '%s\n' "$log" | sed -E "$stamp")"
        b="$(printf '%s\n' "$ps_log" | sed -E "$stamp")"
        if [ "$a" != "$b" ]; then
            _test_failed=1
            echo "    $_test: twins call docker differently" >&2
            diff <(printf '%s\n' "$a") <(printf '%s\n' "$b") | sed 's/^/      /' >&2
        fi
    fi
}

# --- fixtures ---

# Defaults for the keys write_vault writes; a test overrides one before calling it. A value
# of __missing__ leaves the line out.
VAULT_KEY='vault-key-0123456789abcdef0123456789'
MEDIA_KEY='media-key-0123456789abcdef0123456789'
EMAIL_KEY='email-key-0123456789abcdef0123456789'
IDENTITY_KEY='identity-key-secret-0123456789abcdef'
VAPID_KEY='vapid-key-secret-0123456789abcdefghijklmn'
ZITADEL_KEY='masterkey-secret-0123456789abcdef'

# write_vault <dir> <env> [db password] [pseudonym key] [db user]: a complete fixture vault
# for <env> under <dir>/<env>. The postgres password contains `$`, single-quoted as the
# README prescribes, so a report that leaked it would be caught.
write_vault() {
    local dir="$1/$2" env="$2" db_password="${3:-pg\$secret-value}" key="${4-0123456789abcdef0123456789abcdef}" user="${5:-postgres}"
    mkdir -p "$dir"
    printf "POSTGRES_USER=postgres\nPOSTGRES_PASSWORD='pg\$secret-value'\nDATABASE_URL='postgres://postgres:pg\$secret-value@localhost:5432/postit'\n" >"$dir/postgres.env"
    printf "POSTIT__DATABASE__USERNAME=%s\nPOSTIT__DATABASE__PASSWORD='%s'\nPOSTIT__AUDIT__PSEUDONYM_KEY='%s'\n" \
        "$user" "$db_password" "$key" >"$dir/postit.env"
    local name value
    for name in VAULT_KEY MEDIA_KEY EMAIL_KEY; do
        value="${!name}"
        [ "$value" = __missing__ ] && continue
        case "$name" in
            VAULT_KEY) name=POSTIT__VAULT__KEYS__1 ;;
            MEDIA_KEY) name=POSTIT__MEDIA__SIGNING_KEY ;;
            EMAIL_KEY) name=POSTIT__EMAIL__SIGNING_KEY ;;
        esac
        printf "%s='%s'\n" "$name" "$value" >>"$dir/postit.env"
    done
    if [ "$env" = development ]; then
        # Development-only requirements (G1 owns qa and production).
        [ "$IDENTITY_KEY" = __missing__ ] ||
            printf "POSTIT__IDENTITY_MAIL__SIGNING_KEY='%s'\n" "$IDENTITY_KEY" >>"$dir/postit.env"
        [ "$VAPID_KEY" = __missing__ ] ||
            printf "POSTIT__PUSH__VAPID_PRIVATE_KEY='%s'\n" "$VAPID_KEY" >>"$dir/postit.env"
        [ "$ZITADEL_KEY" = __missing__ ] ||
            printf "ZITADEL_MASTERKEY='%s'\n" "$ZITADEL_KEY" >"$dir/zitadel.env"
    fi
}

# --- the runner ---

# Runs every test_* function of the calling file, in file order, each in a subshell.
run_tests() {
    local file="${BASH_SOURCE[1]}" fn failures=0 passed=0
    _base="$(mktemp -d)"
    trap 'rm -rf "$_base"' EXIT
    for fn in $(grep -oE '^test_[A-Za-z0-9_]+' "$file"); do
        # POSTIT_TEST_ONLY=<regex> runs only the matching tests.
        [[ "${fn#test_}" =~ ${POSTIT_TEST_ONLY:-.} ]] || continue
        mkdir -p "$_base/$fn"
        if (
            _test="${fn#test_}"
            _test_failed=0
            work="$(winpath "$_base/$fn")"
            cd "$work" || exit 1
            "$fn"
            exit "$_test_failed"
        ) </dev/null; then
            echo "ok   ${fn#test_}"
            passed=$((passed + 1))
        else
            echo "FAIL ${fn#test_}"
            failures=$((failures + 1))
        fi
    done
    echo "$(basename "$file" .sh): $passed passed, $failures failed$(have_pwsh || echo ' (pwsh not found: Bash halves only)')"
    [ "$failures" = 0 ]
}
