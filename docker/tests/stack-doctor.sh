#!/usr/bin/env bash
# Tests `stack.sh doctor` against throwaway vault folders. Needs bash only: no Docker, no
# network, no real vault. Run from anywhere: docker/tests/stack-doctor.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0

# A complete vault for $1. The postgres password contains `$`, single-quoted as the README
# prescribes, so a mismatch report that leaked it would be caught below.
write_vault() {
    local env="$1" db_password="${2:-pg\$secret-value}" key="${3-0123456789abcdef0123456789abcdef}" user="${4:-postgres}"
    mkdir -p "$tmp/vault/$env"
    printf "POSTGRES_USER=postgres\nPOSTGRES_PASSWORD='pg\$secret-value'\n" >"$tmp/vault/$env/postgres.env"
    printf "POSTIT__DATABASE__USERNAME=%s\nPOSTIT__DATABASE__PASSWORD='%s'\nPOSTIT__AUDIT__PSEUDONYM_KEY='%s'\n" \
        "$user" "$db_password" "$key" >"$tmp/vault/$env/postit.env"
    # The vault master key and the two signing keys. A value of __missing__ leaves the line out.
    local name value
    for name in VAULT_KEY MEDIA_KEY EMAIL_KEY; do
        value="${!name}"
        [ "$value" = __missing__ ] && continue
        case "$name" in
            VAULT_KEY) name=POSTIT__VAULT__KEYS__1 ;;
            MEDIA_KEY) name=POSTIT__MEDIA__SIGNING_KEY ;;
            EMAIL_KEY) name=POSTIT__EMAIL__SIGNING_KEY ;;
        esac
        printf "%s='%s'\n" "$name" "$value" >>"$tmp/vault/$env/postit.env"
    done
    # The identity mail signing key is a development-only requirement (G1 owns qa and production).
    if [ "$env" = development ] && [ "$IDENTITY_KEY" != __missing__ ]; then
        printf "POSTIT__IDENTITY_MAIL__SIGNING_KEY='%s'\n" "$IDENTITY_KEY" >>"$tmp/vault/$env/postit.env"
    fi
    # So is the VAPID key for web push (development.toml turns push on).
    if [ "$env" = development ] && [ "$VAPID_KEY" != __missing__ ]; then
        printf "POSTIT__PUSH__VAPID_PRIVATE_KEY='%s'\n" "$VAPID_KEY" >>"$tmp/vault/$env/postit.env"
    fi
}

# Defaults for the three new keys; a test overrides one before calling write_vault.
VAULT_KEY='vault-key-0123456789abcdef0123456789'
MEDIA_KEY='media-key-0123456789abcdef0123456789'
EMAIL_KEY='email-key-0123456789abcdef0123456789'
IDENTITY_KEY='identity-key-secret-0123456789abcdef'
VAPID_KEY='vapid-key-secret-0123456789abcdefghijklmn'

write_docker_env() {
    cat >"$tmp/docker.env" <<'EOF'
POSTIT_API_URL=https://api.example.com
POSTIT_APP_URL=https://app.example.com
POSTIT_OIDC_ISSUER=https://auth.example.com
POSTIT_OIDC_AUDIENCE=postit
POSTIT_SMTP_HOST=smtp.example.com
POSTIT_MAIL_FROM='postit <noreply@example.com>'
EOF
}

# expect <name> <exit code> "<words the output must contain>" "<values it must not contain>" -- <doctor args...>
# Both lists are space-separated. The default postgres password 'pg$secret' and the default
# pseudonym key are always forbidden.
# Optional VAR=value entries in EXTRA_ENV reach doctor as shell environment.
EXTRA_ENV=()
expect() {
    local name="$1" want_code="$2" wants="$3" forbids="$4" w ok=1
    shift 5
    local out code=0
    out="$(env -u POSTIT_API_URL -u POSTIT_APP_URL -u POSTIT_OIDC_ISSUER -u POSTIT_OIDC_AUDIENCE \
        -u POSTIT_SMTP_HOST -u POSTIT_MAIL_FROM \
        POSTIT_VAULT_DIR="$tmp/vault" POSTIT_DOCKER_ENV_FILE="$tmp/docker.env" \
        ${EXTRA_ENV[@]+"${EXTRA_ENV[@]}"} \
        "$root/stack.sh" doctor "$@" 2>&1)" || code=$?
    [ "$code" = "$want_code" ] || ok=0
    for w in $wants; do [[ "$out" == *"$w"* ]] || ok=0; done
    for w in $forbids 'pg$secret' 0123456789abcdef0123456789abcdef; do [[ "$out" != *"$w"* ]] || ok=0; done
    if [ "$ok" = 0 ]; then
        echo "FAIL $name: exit $code (want $want_code), output:" >&2
        echo "$out" | sed 's/^/    /' >&2
        failures=$((failures + 1))
    else
        echo "ok   $name"
    fi
}

all_six="POSTIT_API_URL POSTIT_APP_URL POSTIT_OIDC_ISSUER POSTIT_OIDC_AUDIENCE POSTIT_SMTP_HOST POSTIT_MAIL_FROM"
short31='abcdefghijklmnopqrstuvwxyz01234'
master='masterkey-secret-0123456789abcdef'

write_zitadel() { printf "ZITADEL_MASTERKEY='%s'\n" "$master" >"$tmp/vault/development/zitadel.env"; }

write_vault qa
expect "complete qa vault" 0 "qa looks complete" "" -- qa

rm -rf "$tmp/vault"; write_vault qa 'other$password'
expect "password mismatch is named, not printed" 1 "POSTIT__DATABASE__PASSWORD" 'other$password' -- qa

rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' '0123456789abcdef0123456789abcdef' otheruser
expect "username mismatch" 1 "POSTIT__DATABASE__USERNAME" "" -- qa

rm -rf "$tmp/vault"; write_vault qa; rm "$tmp/vault/qa/postit.env"
expect "missing vault file" 1 "!ref/vault/qa/postit.env" "" -- qa

rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' tooshortkey
expect "short pseudonym key" 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "tooshortkey" -- qa

rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' ''
expect "empty pseudonym key" 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "" -- qa

# Quotes are not part of the value: 31 characters inside quotes is still too short.
rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' "$short31"
expect "quoted 31-character key is too short" 1 "POSTIT__AUDIT__PSEUDONYM_KEY" "$short31" -- qa

# The vault master key and both signing keys are required outside development, at 32 or more
# characters. Any POSTIT__VAULT__KEYS__<n> satisfies the vault requirement.
for pair in "VAULT_KEY POSTIT__VAULT__KEYS__1" "MEDIA_KEY POSTIT__MEDIA__SIGNING_KEY" "EMAIL_KEY POSTIT__EMAIL__SIGNING_KEY"; do
    var="${pair% *}"; envname="${pair#* }"
    rm -rf "$tmp/vault"; printf -v "$var" '%s' __missing__; write_vault qa
    expect "missing $envname" 1 "$envname" "" -- qa
    rm -rf "$tmp/vault"; printf -v "$var" '%s' "$short31"; write_vault qa
    expect "31-character $envname" 1 "$envname" "$short31" -- qa
    printf -v "$var" '%s' 'restored-key-0123456789abcdef0123456789'
done
VAULT_KEY='vault-key-0123456789abcdef0123456789'; MEDIA_KEY='media-key-0123456789abcdef0123456789'; EMAIL_KEY='email-key-0123456789abcdef0123456789'

rm -rf "$tmp/vault"; VAULT_KEY=__missing__; write_vault qa
printf "POSTIT__VAULT__KEYS__2='vault-key-v2-0123456789abcdef0123456789'\n" >>"$tmp/vault/qa/postit.env"
expect "a vault key at another version satisfies the requirement" 0 "qa looks complete" "" -- qa

rm -rf "$tmp/vault"; VAULT_KEY=__missing__; write_vault qa
printf "POSTIT__VAULT__KEYS__0='vault-key-v0-0123456789abcdef0123456789'\nPOSTIT__VAULT__KEYS__01='vault-key-01-0123456789abcdef0123456789'\n" >>"$tmp/vault/qa/postit.env"
expect "vault key versions 0 and 01 do not satisfy the requirement" 1 "POSTIT__VAULT__KEYS__1" "" -- qa
VAULT_KEY='vault-key-0123456789abcdef0123456789'

rm -rf "$tmp/vault"; write_vault qa
printf 'POSTIT__MAIL__SMTP__USERNAME=relay\n' >>"$tmp/vault/qa/postit.env"
expect "SMTP username without password" 1 "POSTIT__MAIL__SMTP__PASSWORD" "" -- qa

rm -rf "$tmp/vault"; write_vault qa
printf 'POSTIT__MAIL__SMTP__PASSWORD=relay-secret-value\n' >>"$tmp/vault/qa/postit.env"
expect "SMTP password without username" 1 "POSTIT__MAIL__SMTP__USERNAME" "relay-secret-value" -- qa

rm -rf "$tmp/vault"; write_vault qa
printf 'POSTIT__MAIL__SMTP__USERNAME=relay\nPOSTIT__MAIL__SMTP__PASSWORD=relay-secret-value\n' >>"$tmp/vault/qa/postit.env"
expect "complete SMTP credential" 0 "qa looks complete" "relay-secret-value" -- qa

# CRLF line endings (a vault edited on Windows) must not leak a \r into values.
rm -rf "$tmp/vault"; write_vault qa
sed -i 's/$/\r/' "$tmp/vault/qa/postgres.env" "$tmp/vault/qa/postit.env"
expect "CRLF vault" 0 "qa looks complete" "" -- qa

rm -rf "$tmp/vault"; mkdir -p "$tmp/vault/qa"
printf 'POSTGRES_USER="postgres"\nPOSTGRES_PASSWORD="pg-double-quoted"\n' >"$tmp/vault/qa/postgres.env"
printf 'POSTIT__DATABASE__USERNAME="postgres"\nPOSTIT__DATABASE__PASSWORD="pg-double-quoted"\nPOSTIT__AUDIT__PSEUDONYM_KEY="0123456789abcdef0123456789abcdef"\n' >"$tmp/vault/qa/postit.env"
printf 'POSTIT__VAULT__KEYS__1="vault-key-0123456789abcdef0123456789"\nPOSTIT__MEDIA__SIGNING_KEY="media-key-0123456789abcdef0123456789"\nPOSTIT__EMAIL__SIGNING_KEY="email-key-0123456789abcdef0123456789"\n' >>"$tmp/vault/qa/postit.env"
expect "double-quoted values" 0 "qa looks complete" "pg-double-quoted 0123456789abcdef0123456789abcdef" -- qa

# Development also needs zitadel.env with ZITADEL_MASTERKEY.
rm -rf "$tmp/vault"; write_vault development; write_zitadel
expect "complete development vault" 0 "development looks complete" "$master" -- development

printf 'ZITADEL_OTHER=1\n' >"$tmp/vault/development/zitadel.env"
expect "development without master key" 1 "ZITADEL_MASTERKEY" "$master" -- development

rm "$tmp/vault/development/zitadel.env"
expect "development without zitadel.env" 1 "!ref/vault/development/zitadel.env" "" -- development

# The development signing key for the identity mail webhook: set by `cargo xtask zitadel-bootstrap`.
rm -rf "$tmp/vault"; IDENTITY_KEY=__missing__; write_vault development; write_zitadel
expect "development without the identity mail signing key" 1 "POSTIT__IDENTITY_MAIL__SIGNING_KEY cargo xtask zitadel-bootstrap" "" -- development

rm -rf "$tmp/vault"; IDENTITY_KEY=''; write_vault development; write_zitadel
expect "development with an empty identity mail signing key" 1 "POSTIT__IDENTITY_MAIL__SIGNING_KEY cargo xtask zitadel-bootstrap" "" -- development

rm -rf "$tmp/vault"; IDENTITY_KEY='identity-key-secret-0123456789abcdef'; write_vault development; write_zitadel
expect "development with the identity mail signing key" 0 "development looks complete" "identity-key-secret-0123456789abcdef" -- development

IDENTITY_KEY=__missing__; rm -rf "$tmp/vault"; write_vault qa
expect "qa without the identity mail signing key" 0 "qa looks complete" "" -- qa
IDENTITY_KEY='identity-key-secret-0123456789abcdef'

# The development VAPID key for web push: set by `cargo xtask vapid-keygen`.
rm -rf "$tmp/vault"; VAPID_KEY=__missing__; write_vault development; write_zitadel
expect "development without the VAPID key" 1 "POSTIT__PUSH__VAPID_PRIVATE_KEY cargo xtask vapid-keygen" "" -- development

rm -rf "$tmp/vault"; VAPID_KEY=''; write_vault development; write_zitadel
expect "development with an empty VAPID key" 1 "POSTIT__PUSH__VAPID_PRIVATE_KEY cargo xtask vapid-keygen" "" -- development

rm -rf "$tmp/vault"; VAPID_KEY='vapid-key-secret-0123456789abcdefghijklmn'; write_vault development; write_zitadel
expect "development with the VAPID key" 0 "development looks complete" "vapid-key-secret-0123456789abcdefghijklmn" -- development

VAPID_KEY=__missing__; rm -rf "$tmp/vault"; write_vault qa
expect "qa without the VAPID key" 0 "qa looks complete" "" -- qa
VAPID_KEY='vapid-key-secret-0123456789abcdefghijklmn'

# Production: the six variables docker-compose.production.yml requires.
rm -rf "$tmp/vault"; write_vault production; rm -f "$tmp/docker.env"
expect "production without deployment values" 1 "$all_six" "" -- production

write_docker_env
expect "complete production" 0 "production looks complete" "" -- production

rm -f "$tmp/docker.env"
EXTRA_ENV=(POSTIT_API_URL=https://api.example.com POSTIT_APP_URL=https://app.example.com
    POSTIT_OIDC_ISSUER=https://auth.example.com POSTIT_OIDC_AUDIENCE=postit
    POSTIT_SMTP_HOST=smtp.example.com 'POSTIT_MAIL_FROM=postit <noreply@example.com>')
expect "production with values from the shell" 0 "production looks complete" "" -- production
EXTRA_ENV=()

[ "$failures" = 0 ] || { echo "$failures doctor test(s) failed" >&2; exit 1; }
echo "all doctor tests passed"
