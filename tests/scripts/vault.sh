#!/usr/bin/env bash
# Tests vault.sh and vault.ps1 against throwaway vault folders and archives, with a real
# 7-Zip (skipped when none is installed, except in CI). The password reaches the scripts
# on standard input, as a person types it. Run: bash tests/scripts/vault.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The 7-Zip the scripts would find, in the same order.
sz="${POSTIT_7Z:-}"
[ -n "$sz" ] || sz="$(command -v 7z || command -v 7za || true)"
[ -n "$sz" ] || [ ! -f "C:/Program Files/7-Zip/7z.exe" ] || sz="C:/Program Files/7-Zip/7z.exe"
if [ -z "$sz" ]; then
    if [ -n "${CI:-}" ]; then
        echo "vault: no 7-Zip found, and CI must run these tests" >&2
        exit 1
    fi
    echo "vault: skipped, no 7-Zip found (install p7zip-full or 7-Zip)"
    exit 0
fi
export POSTIT_7Z="$sz"

password='test-vault-pass'

# A vault with every environment and a .gitkeep, under $work/ref, and the archive beside it.
setup_vault() {
    ref="$work/ref"
    vault="$ref/vault"
    archive="$ref/vault.7z"
    write_vault "$vault" development
    write_vault "$vault" qa
    write_vault "$vault" production
    : >"$vault/.gitkeep"
    export POSTIT_VAULT_DIR="$vault" POSTIT_VAULT_ARCHIVE="$archive"
}

# The test's own 7-Zip calls; the password on the command line is fine here.
pack_directly() { (cd "$ref" && "$sz" a -p"$password" -mhe=on '-xr!.gitkeep' vault.7z vault >/dev/null); }
extract() { "$sz" x -p"${2:-$password}" -o"$1" "$archive" >/dev/null; }

sha() { sha256sum "$1" | cut -d' ' -f1; }

test_help_layout() {
    twin vault help
    check "exit 0" is "$code" 0
    check_layout vault
    twin vault
    check "a bare call exits 0" is "$code" 0
}

test_unknown_command_is_exit_1() {
    twin vault frobnicate
    check "exit 1" is "$code" 1
    err_has "unknown command: frobnicate"
}

test_missing_7z_names_the_download() {
    setup_vault
    export POSTIT_7Z="$work/no-such-7z"
    twin vault pack </dev/null
    check "exit 1" is "$code" 1
    err_has "7-zip.org"
}

test_pack_then_check_is_equal() {
    setup_vault
    pack_directly
    printf "EXTRA='x'\n" >>"$vault/qa/postit.env"
    printf '%s\n' "$password" | twin vault pack
    check "pack exits 0 ($err $ps_err)" is "$code" 0
    printf '%s\n' "$password" | twin vault check
    check "check exits 0 ($out $err)" is "$code" 0
    out_has "matches"
}

test_check_reports_a_changed_file() {
    setup_vault
    pack_directly
    printf "EXTRA='changed-secret-value'\n" >>"$vault/qa/postit.env"
    printf 'NEW=1\n' >"$vault/development/new.env"
    rm "$vault/production/postgres.env"
    printf '%s\n' "$password" | twin vault check
    check "exit 2 ($err)" is "$code" 2
    out_has "qa/postit.env"
    out_has "development/new.env"
    out_has "production/postgres.env"
    check "unchanged files are not listed" lacks "$out$ps_out" "qa/postgres.env"
    check "no value is printed" lacks "$out$err$ps_out$ps_err" "changed-secret-value"
}

# One file on each side: no list of one may be glued into another.
test_check_with_one_file_each_side() {
    ref="$work/ref" vault="$work/ref/vault" archive="$work/ref/vault.7z"
    export POSTIT_VAULT_DIR="$vault" POSTIT_VAULT_ARCHIVE="$archive"
    mkdir -p "$vault/qa"
    printf 'A=1\n' >"$vault/qa/a.env"
    pack_directly
    rm "$vault/qa/a.env"
    printf 'B=1\n' >"$vault/qa/b.env"
    printf '%s\n' "$password" | twin vault check
    check "exit 2" is "$code" 2
    local text
    for text in "$out" "${ps_out:-$out}"; do
        check "names qa/a.env on its own line ($text)" matches "$text" "(^|"$'\n'")only in the archive: qa/a.env("$'\n'"|$)"
        check "names qa/b.env on its own line ($text)" matches "$text" "(^|"$'\n'")only in [^"$'\n'"]*: qa/b.env("$'\n'"|$)"
    done
}

test_unpack_refuses_an_existing_vault() {
    setup_vault
    pack_directly
    printf '%s\n' "$password" | twin vault unpack
    check "exit 1" is "$code" 1
    err_has "already holds"
}

test_unpack_into_an_empty_vault() {
    setup_vault
    pack_directly
    twin_setup() { rm -rf "${vault:?}"/*; }
    twin_after() { [ -f "$vault/qa/postit.env" ] || echo "$1" >>"$work/.missing"; }
    printf '%s\n' "$password" | twin vault unpack
    check "exit 0 ($err $ps_err)" is "$code" 0
    check "every twin restored the files (missing after: $(cat "$work/.missing" 2>/dev/null))" test ! -e "$work/.missing"
    check "nothing left behind" is "$(cd "$vault" && find . -name '.unpack*')" ""
}

test_unpack_with_a_wrong_password_changes_nothing() {
    setup_vault
    pack_directly
    twin_setup() { rm -rf "${vault:?}"/*; }
    printf 'wrong-password\n' | twin vault unpack
    check "exit 1" is "$code" 1
    check "nothing unpacked" is "$(find "$vault" -mindepth 1 ! -name .gitkeep)" ""
}

test_unpack_force_overwrites() {
    setup_vault
    pack_directly
    twin_setup() { printf 'BROKEN=1\n' >"$vault/qa/postit.env"; }
    twin_after() { grep -q POSTIT__DATABASE__USERNAME "$vault/qa/postit.env" || echo "$1" >>"$work/.broken"; }
    printf '%s\n' "$password" | twin vault unpack --force
    check "exit 0 ($err $ps_err)" is "$code" 0
    check "every twin restored the file (still broken after: $(cat "$work/.broken" 2>/dev/null))" test ! -e "$work/.broken"
}

test_pack_leaves_out_gitkeep() {
    setup_vault
    pack_directly
    printf '%s\n' "$password" | twin vault pack
    check "exit 0 ($err $ps_err)" is "$code" 0
    extract "$work/out"
    check "files sit under vault/" test -f "$work/out/vault/development/postit.env"
    check "no .gitkeep" is "$(find "$work/out" -name .gitkeep)" ""
}

test_pack_includes_production() {
    setup_vault
    pack_directly
    printf '%s\n' "$password" | twin vault pack
    extract "$work/out"
    check "production/postit.env is packed" test -f "$work/out/vault/production/postit.env"
    check "production/postgres.env is packed" test -f "$work/out/vault/production/postgres.env"
}

# A mistyped password must not re-key the archive.
test_pack_with_a_wrong_password_keeps_the_archive() {
    setup_vault
    pack_directly
    local before
    before="$(sha "$archive")"
    printf 'wrong-password\n' | twin vault pack
    check "exit 1" is "$code" 1
    err_has "does not open"
    check "the archive is byte-for-byte unchanged" is "$(sha "$archive")" "$before"
    check "it still opens with the old password" extract "$work/out"
}

test_pack_new_password_asks_twice() {
    setup_vault
    pack_directly
    local before
    before="$(sha "$archive")"
    printf 'new-one\nnew-two\n' | twin vault pack --new-password
    check "two different answers: exit 1" is "$code" 1
    err_has "did not match"
    check "two different answers: the archive is unchanged" is "$(sha "$archive")" "$before"
    printf 'new-pass\nnew-pass\n' | twin vault pack --new-password
    check "the same answer twice: exit 0 ($err $ps_err)" is "$code" 0
    check "the archive opens with the new password" extract "$work/out" new-pass
}

test_first_pack_asks_twice() {
    setup_vault
    twin_setup() { rm -f "$archive"; }
    printf 'first-pass\nfirst-pass\n' | twin vault pack
    check "exit 0 ($err $ps_err)" is "$code" 0
    check "the archive opens with that password" extract "$work/out" first-pass
}

test_pack_failure_keeps_the_old_archive() {
    setup_vault
    pack_directly
    local before
    before="$(sha "$archive")"
    export TWIN_FAKES=7z
    printf '%s\n' "$password" | twin vault pack
    check "exit 1" is "$code" 1
    check "the old archive is byte-for-byte unchanged" is "$(sha "$archive")" "$before"
    check "no temporary archive is left ($(ls -A "$ref"))" is "$(ls -A "$ref" | tr '\n' ' ')" "vault vault.7z "
}

# 7-Zip reads the password from its standard input; it never becomes an argument.
test_the_password_is_never_an_argument() {
    setup_vault
    pack_directly
    export TWIN_FAKES=7z
    printf '%s\n' "$password" | twin vault pack
    printf '%s\n' "$password" | twin vault check
    check "7z was called (log: $log)" has "$log" "7z "
    check "no -p<password> argument (log: $log)" lacks "$log" '-p***'
    check "the password is not in the log" lacks "$log$ps_log" "$password"
}

test_check_removes_its_temporary_copy() {
    setup_vault
    pack_directly
    export TMPDIR="$work/tmpdir"
    mkdir -p "$TMPDIR"
    twin_after() { [ -z "$(ls -A "$TMPDIR")" ] || echo "$1" >>"$work/.left"; }
    printf '%s\n' "$password" | twin vault check
    check "exit 0 ($err)" is "$code" 0
    check "nothing left in TMPDIR (by: $(cat "$work/.left" 2>/dev/null))" test ! -e "$work/.left"
    printf 'wrong-password\n' | twin vault check
    check "a failed extraction is exit 1" is "$code" 1
    check "nothing left in TMPDIR after a failure (by: $(cat "$work/.left" 2>/dev/null))" test ! -e "$work/.left"
}

run_tests
