#!/usr/bin/env bash
# Tests cert.sh and cert.ps1 with a throwaway certificate folder. Needs openssl; `trust` is
# never run here (it changes the machine's trust store). Run: bash tests/scripts/cert.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

openssl="$(command -v openssl || true)"
[ -n "$openssl" ] || [ ! -f "C:/Program Files/Git/usr/bin/openssl.exe" ] || openssl="C:/Program Files/Git/usr/bin/openssl.exe"
if [ -z "$openssl" ]; then
    if [ -n "${CI:-}" ]; then
        echo "cert: no openssl found, and CI must run these tests" >&2
        exit 1
    fi
    echo "cert: skipped, no openssl found"
    exit 0
fi

sha() { sha256sum "$1" | cut -d' ' -f1; }
verifies() { "$openssl" verify -CAfile "$1" "$2" >/dev/null 2>&1; }

test_help_layout() {
    twin cert help
    check "exit 0" is "$code" 0
    check_layout cert
}

test_unknown_command_is_exit_1() {
    twin cert frobnicate
    check "exit 1" is "$code" 1
    err_has "unknown command: frobnicate"
}

test_gen_writes_the_four_files() {
    export POSTIT_CERT_DIR="$work/certs"
    twin cert gen
    check "exit 0 ($err $ps_err)" is "$code" 0
    local f
    for f in postit-dev-ca.key postit-dev-ca.crt postit.local.key postit.local.crt; do
        check "$f written" test -s "$POSTIT_CERT_DIR/$f"
    done
    check "no CSR left behind" test ! -e "$POSTIT_CERT_DIR/postit.local.csr"
    check "gen does not trust" lacks "$out$ps_out" "Root store"
}

test_gen_keeps_an_existing_ca() {
    export POSTIT_CERT_DIR="$work/certs"
    twin cert gen
    local before
    before="$(sha "$POSTIT_CERT_DIR/postit-dev-ca.crt")"
    twin cert gen
    check "exit 0" is "$code" 0
    check "the CA is unchanged" is "$(sha "$POSTIT_CERT_DIR/postit-dev-ca.crt")" "$before"
    out_has "reusing the existing dev CA"
}

test_leaf_has_the_san_and_chains_to_the_ca() {
    export POSTIT_CERT_DIR="$work/certs"
    twin cert gen
    local san
    san="$("$openssl" x509 -noout -ext subjectAltName -in "$POSTIT_CERT_DIR/postit.local.crt")"
    check "DNS:postit.local ($san)" has "$san" "DNS:postit.local"
    check "DNS:*.postit.local ($san)" has "$san" "DNS:*.postit.local"
    check "signed by the dev CA" verifies "$POSTIT_CERT_DIR/postit-dev-ca.crt" "$POSTIT_CERT_DIR/postit.local.crt"
}

run_tests
