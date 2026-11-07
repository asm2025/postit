#!/usr/bin/env bash
# Tests doctor.sh and doctor.ps1 against throwaway vault folders and the fake docker. No
# Docker, no network, no real vault. Run: bash tests/scripts/doctor.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

all_six="POSTIT_API_URL POSTIT_APP_URL POSTIT_OIDC_ISSUER POSTIT_OIDC_AUDIENCE POSTIT_SMTP_HOST POSTIT_MAIL_FROM"
short31='abcdefghijklmnopqrstuvwxyz01234'

# Doctor reads only the throwaway vault and docker/.env, checks the vault only (the
# machine checks have their own tests), and never sees deployment values from this shell.
setup() {
    export POSTIT_VAULT_DIR="$work/vault" POSTIT_DOCKER_ENV_FILE="$work/docker.env" POSTIT_DOCTOR_OFFLINE=1
    export POSTIT_DOCTOR_ONLY="${POSTIT_DOCTOR_ONLY-vault,compose-variables}"
    unset $all_six
}

write_docker_env() {
    cat >"$work/docker.env" <<'EOF'
POSTIT_API_URL=https://api.example.com
POSTIT_APP_URL=https://app.example.com
POSTIT_OIDC_ISSUER=https://auth.example.com
POSTIT_OIDC_AUDIENCE=postit
POSTIT_SMTP_HOST=smtp.example.com
POSTIT_MAIL_FROM='postit <noreply@example.com>'
EOF
}

# expect <exit code> "<words the output must contain>" "<values it must not contain>" <doctor args...>
# Both lists are space-separated. The fixture postgres password and pseudonym key are
# always forbidden.
expect() {
    local want_code="$1" wants="$2" forbids="$3" w
    shift 3
    twin doctor "$@"
    check "exit $code, want $want_code (out: $out)" is "$code" "$want_code"
    for w in $wants; do out_has "$w"; done
    for w in $forbids 'pg$secret' 0123456789abcdef0123456789abcdef; do
        check "never prints '$w'" lacks "$out$err$ps_out$ps_err" "$w"
    done
}

fresh() { rm -rf "$work/vault"; }

test_help_layout() {
    twin doctor help
    check "exit 0" is "$code" 0
    check_layout doctor
}

test_unknown_environment_is_exit_1() {
    twin doctor check staging
    check "exit 1" is "$code" 1
    err_has "staging"
}

test_complete_qa_vault() {
    setup; write_vault "$work/vault" qa
    expect 0 "PASS" "" check qa
}

test_password_mismatch_is_named_not_printed() {
    setup; write_vault "$work/vault" qa 'other$password'
    expect 1 "FAIL POSTIT__DATABASE__PASSWORD" 'other$password' check qa
}

test_username_mismatch() {
    setup; write_vault "$work/vault" qa 'pg$secret-value' '0123456789abcdef0123456789abcdef' otheruser
    expect 1 "POSTIT__DATABASE__USERNAME" "" check qa
}

test_missing_vault_file() {
    setup; write_vault "$work/vault" qa; rm "$work/vault/qa/postit.env"
    expect 1 "!ref/vault/qa/postit.env unpack" "" check qa
}

test_short_and_empty_pseudonym_keys() {
    setup; write_vault "$work/vault" qa 'pg$secret-value' tooshortkey
    expect 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "tooshortkey" check qa
    fresh; write_vault "$work/vault" qa 'pg$secret-value' ''
    expect 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "" check qa
    # Quotes are not part of the value: 31 characters inside quotes is still too short.
    fresh; write_vault "$work/vault" qa 'pg$secret-value' "$short31"
    expect 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "$short31" check qa
}

# The vault master key and both signing keys are required outside development, at 32 or
# more characters.
test_vault_and_signing_keys_outside_development() {
    setup
    local pair var envname
    for pair in "VAULT_KEY POSTIT__VAULT__KEYS__1" "MEDIA_KEY POSTIT__MEDIA__SIGNING_KEY" "EMAIL_KEY POSTIT__EMAIL__SIGNING_KEY"; do
        var="${pair% *}"
        envname="${pair#* }"
        fresh; printf -v "$var" '%s' __missing__; write_vault "$work/vault" qa
        expect 1 "$envname" "" check qa
        fresh; printf -v "$var" '%s' "$short31"; write_vault "$work/vault" qa
        expect 1 "$envname" "$short31" check qa
        printf -v "$var" '%s' 'restored-key-0123456789abcdef0123456789'
    done
}

test_a_vault_key_at_another_version_satisfies_the_requirement() {
    setup; VAULT_KEY=__missing__; write_vault "$work/vault" qa
    printf "POSTIT__VAULT__KEYS__2='vault-key-v2-0123456789abcdef0123456789'\n" >>"$work/vault/qa/postit.env"
    expect 0 "PASS" "" check qa
}

test_vault_key_versions_0_and_01_do_not_count() {
    setup; VAULT_KEY=__missing__; write_vault "$work/vault" qa
    printf "POSTIT__VAULT__KEYS__0='vault-key-v0-0123456789abcdef0123456789'\nPOSTIT__VAULT__KEYS__01='vault-key-01-0123456789abcdef0123456789'\n" >>"$work/vault/qa/postit.env"
    expect 1 "POSTIT__VAULT__KEYS__1" "" check qa
}

test_smtp_credential_pairs() {
    setup; write_vault "$work/vault" qa
    printf 'POSTIT__MAIL__SMTP__USERNAME=relay\n' >>"$work/vault/qa/postit.env"
    expect 1 "POSTIT__MAIL__SMTP__PASSWORD" "" check qa
    fresh; write_vault "$work/vault" qa
    printf 'POSTIT__MAIL__SMTP__PASSWORD=relay-secret-value\n' >>"$work/vault/qa/postit.env"
    expect 1 "POSTIT__MAIL__SMTP__USERNAME" "relay-secret-value" check qa
    fresh; write_vault "$work/vault" qa
    printf 'POSTIT__MAIL__SMTP__USERNAME=relay\nPOSTIT__MAIL__SMTP__PASSWORD=relay-secret-value\n' >>"$work/vault/qa/postit.env"
    expect 0 "PASS" "relay-secret-value" check qa
}

# CRLF line endings (a vault edited on Windows) must not leak a \r into values.
test_crlf_vault() {
    setup; write_vault "$work/vault" qa
    sed -i 's/$/\r/' "$work/vault/qa/postgres.env" "$work/vault/qa/postit.env"
    expect 0 "PASS" "" check qa
}

test_double_quoted_values() {
    setup; mkdir -p "$work/vault/qa"
    printf 'POSTGRES_USER="postgres"\nPOSTGRES_PASSWORD="pg-double-quoted"\n' >"$work/vault/qa/postgres.env"
    printf 'POSTIT__DATABASE__USERNAME="postgres"\nPOSTIT__DATABASE__PASSWORD="pg-double-quoted"\nPOSTIT__AUDIT__PSEUDONYM_KEY="0123456789abcdef0123456789abcdef"\n' >"$work/vault/qa/postit.env"
    printf 'POSTIT__VAULT__KEYS__1="vault-key-0123456789abcdef0123456789"\nPOSTIT__MEDIA__SIGNING_KEY="media-key-0123456789abcdef0123456789"\nPOSTIT__EMAIL__SIGNING_KEY="email-key-0123456789abcdef0123456789"\n' >>"$work/vault/qa/postit.env"
    expect 0 "PASS" "pg-double-quoted" check qa
}

test_development_needs_the_zitadel_master_key() {
    setup; write_vault "$work/vault" development
    expect 0 "PASS" "$ZITADEL_KEY" check development
    printf 'ZITADEL_OTHER=1\n' >"$work/vault/development/zitadel.env"
    expect 1 "ZITADEL_MASTERKEY" "$ZITADEL_KEY" check development
    rm "$work/vault/development/zitadel.env"
    expect 1 "!ref/vault/development/zitadel.env" "" check development
}

# Set by `cargo xtask zitadel-bootstrap`; development only (G1 owns qa and production).
test_development_identity_mail_signing_key() {
    setup; IDENTITY_KEY=__missing__; write_vault "$work/vault" development
    expect 1 "POSTIT__IDENTITY_MAIL__SIGNING_KEY cargo xtask zitadel-bootstrap" "" check development
    fresh; IDENTITY_KEY=''; write_vault "$work/vault" development
    expect 1 "POSTIT__IDENTITY_MAIL__SIGNING_KEY cargo xtask zitadel-bootstrap" "" check development
    fresh; IDENTITY_KEY='identity-key-secret-0123456789abcdef'; write_vault "$work/vault" development
    expect 0 "PASS" "identity-key-secret-0123456789abcdef" check development
    IDENTITY_KEY=__missing__; fresh; write_vault "$work/vault" qa
    expect 0 "PASS" "" check qa
}

# Set by `cargo xtask vapid-keygen`; development only.
test_development_vapid_key() {
    setup; VAPID_KEY=__missing__; write_vault "$work/vault" development
    expect 1 "POSTIT__PUSH__VAPID_PRIVATE_KEY cargo xtask vapid-keygen" "" check development
    fresh; VAPID_KEY=''; write_vault "$work/vault" development
    expect 1 "POSTIT__PUSH__VAPID_PRIVATE_KEY cargo xtask vapid-keygen" "" check development
    fresh; VAPID_KEY='vapid-key-secret-0123456789abcdefghijklmn'; write_vault "$work/vault" development
    expect 0 "PASS" "vapid-key-secret-0123456789abcdefghijklmn" check development
    VAPID_KEY=__missing__; fresh; write_vault "$work/vault" qa
    expect 0 "PASS" "" check qa
}

# Production: the six variables docker-compose.production.yml requires.
test_production_compose_variables() {
    setup; write_vault "$work/vault" production
    expect 1 "compose-variables $all_six" "" check production
    write_docker_env
    expect 0 "compose-variables" "" check production
}

test_production_compose_variables_from_the_shell() {
    setup; write_vault "$work/vault" production
    export POSTIT_API_URL=https://api.example.com POSTIT_APP_URL=https://app.example.com \
        POSTIT_OIDC_ISSUER=https://auth.example.com POSTIT_OIDC_AUDIENCE=postit \
        POSTIT_SMTP_HOST=smtp.example.com 'POSTIT_MAIL_FROM=postit <noreply@example.com>'
    expect 0 "PASS" "" check production
}

# --- the shape of the report ---

test_doctor_never_prints_a_value_with_dollar_and_equals() {
    setup; write_vault "$work/vault" qa "p\$a=ss'\"w"
    expect 1 "POSTIT__DATABASE__PASSWORD" "p\$a=ss'\"w p\$a=ss" check qa
}

test_rows_have_the_fixed_shape() {
    POSTIT_DOCTOR_ONLY=''; setup; write_vault "$work/vault" development; rm "$work/vault/development/zitadel.env"
    twin doctor check development
    local line text
    for text in "$out" "${ps_out:-$out}"; do
        check "some rows" has "$text" "  vault  "
        while IFS= read -r line; do
            check "row shape: '$line'" matches "$line" '^(PASS|WARN|FAIL|SKIP)  [a-z-]+  '
        done <<<"$text"
    done
}

test_fail_rows_name_a_fix() {
    POSTIT_DOCTOR_ONLY=''; setup; write_vault "$work/vault" development 'other$password'
    twin doctor check development
    local line text
    for text in "$out" "${ps_out:-$out}"; do
        check "a FAIL row" matches "$text" '(^|'$'\n'')FAIL  '
        while IFS= read -r line; do
            case "$line" in FAIL* | WARN*) check "names a fix: '$line'" has "$line" " -> " ;; esac
        done <<<"$text"
    done
}

test_bare_call_checks_development() {
    POSTIT_DOCTOR_ONLY=vault; setup; write_vault "$work/vault" development
    twin doctor
    check "exit 0 ($out)" is "$code" 0
    out_has "PASS  vault  development"
}

test_qa_runs_vault_checks_only() {
    POSTIT_DOCTOR_ONLY=''; setup; write_vault "$work/vault" qa
    twin doctor check qa
    check "exit 0 ($out)" is "$code" 0
    local name
    for name in tools certificate hosts postgres migration identity web-packages compose-variables; do
        check "no $name row" lacks "$out" "  $name  "
    done
    check "no docker call" is "$log" ""
}

test_offline_skips_docker_and_network() {
    POSTIT_DOCTOR_ONLY=''; setup; write_vault "$work/vault" development
    twin doctor check development
    out_has "SKIP  postgres  (offline)"
    out_has "SKIP  identity  (offline)"
    check "no docker call" is "$log" ""
}

test_migration_checksum_mismatch_names_the_reset() {
    POSTIT_DOCTOR_ONLY=migration; setup; unset POSTIT_DOCTOR_OFFLINE
    export FAKE_DOCKER_STDOUT=00ff
    twin doctor check development
    check "exit 1" is "$code" 1
    out_has "FAIL  migration"
    out_has "drop development"
}

test_migration_checksum_match_passes() {
    POSTIT_DOCTOR_ONLY=migration; setup; unset POSTIT_DOCTOR_OFFLINE
    FAKE_DOCKER_STDOUT="$(sha384sum <"$root/backend/crates/data/migrations/0001_init.sql" | cut -d' ' -f1)"
    export FAKE_DOCKER_STDOUT
    twin doctor check development
    check "exit 0 ($out)" is "$code" 0
    out_has "PASS  migration"
}

test_migration_not_applied_yet_warns() {
    POSTIT_DOCTOR_ONLY=migration; setup; unset POSTIT_DOCTOR_OFFLINE
    twin doctor check development
    check "exit 0 ($out)" is "$code" 0
    out_has "WARN  migration"
}

# A fresh machine without Docker still gets every row.
test_no_docker_still_reports_every_check() {
    POSTIT_DOCTOR_ONLY='postgres,migration,zitadel-bootstrap'; setup; unset POSTIT_DOCTOR_OFFLINE
    export TWIN_DOCKER="$work/no-such-docker" POSTIT_LOCAL_CONFIG="$work/local.toml"
    : >"$work/local.toml"
    twin doctor check development
    check "exit 1" is "$code" 1
    out_has "FAIL  postgres  docker is not available"
    out_has "SKIP  migration  (needs postgres)"
    out_has "PASS  zitadel-bootstrap"
}

test_zitadel_bootstrap_and_web_packages_are_files() {
    POSTIT_DOCTOR_ONLY='zitadel-bootstrap,web-packages'; setup
    export POSTIT_LOCAL_CONFIG="$work/local.toml" POSTIT_WEB_MODULES="$work/node_modules"
    twin doctor check development
    check "exit 1" is "$code" 1
    out_has "FAIL  zitadel-bootstrap"
    out_has "cargo xtask zitadel-bootstrap"
    out_has "FAIL  web-packages"
    out_has "pnpm install"
    : >"$work/local.toml"; mkdir -p "$work/node_modules"
    twin doctor check development
    check "exit 0 ($out)" is "$code" 0
    out_has "PASS  zitadel-bootstrap"
    out_has "PASS  web-packages"
}

test_certificate_missing_names_cert_setup() {
    POSTIT_DOCTOR_ONLY='certificate,certificate-trusted'; setup
    export POSTIT_CERT_DIR="$work/certs"
    twin doctor check development
    check "exit 1" is "$code" 1
    out_has "FAIL  certificate"
    out_has "setup"
    out_has "SKIP  certificate-trusted  (needs certificate)"
}

run_tests
