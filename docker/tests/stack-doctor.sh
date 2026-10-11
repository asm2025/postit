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
    local env="$1" db_password="${2:-pg\$secret-value}" key="${3:-0123456789abcdef0123456789abcdef}"
    mkdir -p "$tmp/vault/$env"
    printf "POSTGRES_USER=postgres\nPOSTGRES_PASSWORD='pg\$secret-value'\n" >"$tmp/vault/$env/postgres.env"
    printf "POSTIT__DATABASE__USERNAME=postgres\nPOSTIT__DATABASE__PASSWORD='%s'\nPOSTIT__AUDIT__PSEUDONYM_KEY='%s'\n" \
        "$db_password" "$key" >"$tmp/vault/$env/postit.env"
}

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

# expect <name> <exit code> <text the output must contain> -- <doctor args...>
expect() {
    local name="$1" want_code="$2" want_text="$3"
    shift 4
    local out code=0
    out="$(env -u POSTIT_API_URL -u POSTIT_APP_URL -u POSTIT_OIDC_ISSUER -u POSTIT_OIDC_AUDIENCE \
        -u POSTIT_SMTP_HOST -u POSTIT_MAIL_FROM \
        POSTIT_VAULT_DIR="$tmp/vault" POSTIT_DOCKER_ENV_FILE="$tmp/docker.env" \
        "$root/stack.sh" doctor "$@" 2>&1)" || code=$?
    if [ "$code" != "$want_code" ] || [[ "$out" != *"$want_text"* ]] || [[ "$out" == *'pg$secret'* ]]; then
        echo "FAIL $name: exit $code (want $want_code), output:" >&2
        echo "$out" | sed 's/^/    /' >&2
        failures=$((failures + 1))
    else
        echo "ok   $name"
    fi
}

write_vault qa
expect "complete qa vault" 0 "qa looks complete" -- qa

rm -rf "$tmp/vault"; write_vault qa 'other$password'
expect "password mismatch is named, not printed" 1 "POSTIT__DATABASE__PASSWORD" -- qa

rm -rf "$tmp/vault"; write_vault qa; rm "$tmp/vault/qa/postit.env"
expect "missing vault file" 1 "!ref/vault/qa/postit.env" -- qa

rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' short
expect "short pseudonym key" 1 "POSTIT__AUDIT__PSEUDONYM_KEY" -- qa

rm -rf "$tmp/vault"; write_vault production; rm -f "$tmp/docker.env"
expect "production without deployment values" 1 "POSTIT_OIDC_ISSUER" -- production

write_docker_env
expect "complete production" 0 "production looks complete" -- production

rm -rf "$tmp/vault"; write_vault qa
printf "POSTIT__MAIL__SMTP__USERNAME=relay\n" >>"$tmp/vault/qa/postit.env"
expect "half an SMTP credential" 1 "POSTIT__MAIL__SMTP__PASSWORD" -- qa

[ "$failures" = 0 ] || { echo "$failures doctor test(s) failed" >&2; exit 1; }
echo "all doctor tests passed"
