#!/usr/bin/env bash
# Is this machine ready, and is an environment's vault complete? Read-only. Twin: doctor.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=doctor
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
doctor - check that this machine is ready and an environment's vault is complete.

USAGE
  ./doctor.sh [check] [environment]

COMMANDS
  check     Run the checks (the default) and print one row per finding:
            PASS, WARN, FAIL, or SKIP, then the check's name. A FAIL or WARN row
            ends with -> and the fix. Read-only: it writes nothing, starts
            nothing, asks nothing, and never prints a secret
  help      Show this help (also -h, --help)

  development (the default) checks, in order: tools, vault, certificate,
  certificate-trusted, hosts, postgres, migration, identity, zitadel-bootstrap,
  web-packages. qa checks the vault only; production the vault and
  compose-variables (no Docker, no network).

OPTIONS
  environment   development (the default), qa, or production

  POSTIT_DOCTOR_ONLY=vault,hosts   run only the named checks
  POSTIT_DOCTOR_OFFLINE=1          skip the checks that need Docker or the network

EXAMPLES
  ./doctor.sh                           Is this machine ready for development?
  ./doctor.sh check qa                  Is the qa vault complete?
  POSTIT_DOCTOR_ONLY=migration ./doctor.sh

EXIT CODES
  0  no FAIL row (WARN rows allowed)
  1  at least one FAIL row, or a usage error

MORE
  docs/scripts.md                       every operator script and the first-run order
  README.md, "Environment files and secrets"
EOF
}

environment=development
case "${1:-check}" in
    help | -h | --help)
        usage
        exit 0
        ;;
    check) [ $# -eq 0 ] || shift ;;
esac
if [ $# -gt 0 ]; then
    environment="$(parse_environment "$1")"
    [ -n "$environment" ] || {
        usage >&2
        die "unknown command or environment: $1"
    }
    shift
fi
[ $# -eq 0 ] || die "unexpected argument: $1"

vault="$(vault_dir)/$environment"
cert_dir="${POSTIT_CERT_DIR:-$POSTIT_ROOT/docker/shared/nginx/certs}"
docker_env_file="${POSTIT_DOCKER_ENV_FILE:-$POSTIT_ROOT/docker/.env}"
only=",${POSTIT_DOCTOR_ONLY:-},"
only="${only// /,}"
offline="${POSTIT_DOCTOR_OFFLINE:-}"
failed=0
declare -A result=()

# row <status> <check> <finding> [fix]
row() {
    local line="$1  $2  $3"
    [ -z "${4:-}" ] || line="$line -> $4"
    printf '%s\n' "$line"
    case "$1" in
        FAIL) failed=1; result[$2]=FAIL ;;
        WARN) [ "${result[$2]:-}" = FAIL ] || result[$2]=WARN ;;
        SKIP) [ -n "${result[$2]:-}" ] || result[$2]=SKIP ;;
        PASS) [ -n "${result[$2]:-}" ] || result[$2]=PASS ;;
    esac
}

# run <check> [needs <prerequisite>] [network]: runs check_<name> unless POSTIT_DOCTOR_ONLY
# leaves it out, a prerequisite that ran did not pass, or it needs Docker or the network
# while offline.
run() {
    local name="$1" needs="" net=0
    shift
    while [ $# -gt 0 ]; do
        case "$1" in
            needs) needs="$2"; shift 2 ;;
            network) net=1; shift ;;
        esac
    done
    [ "$only" = ",," ] || [[ "$only" == *",$name,"* ]] || return 0
    if [ "$net" = 1 ] && [ -n "$offline" ]; then
        row SKIP "$name" "(offline)"
        return 0
    fi
    if [ -n "$needs" ] && [ -n "${result[$needs]:-}" ] && [ "${result[$needs]}" != PASS ] && [ "${result[$needs]}" != WARN ]; then
        row SKIP "$name" "(needs $needs)"
        return 0
    fi
    "check_${name//-/_}"
}

have() { command -v "$1" >/dev/null 2>&1; }

on_windows() { [ -n "${SYSTEMROOT:-}" ] || [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* ]]; }

openssl_cmd() {
    if have openssl; then printf openssl; elif [ -f "C:/Program Files/Git/usr/bin/openssl.exe" ]; then
        printf '%s' "C:/Program Files/Git/usr/bin/openssl.exe"
    fi
}

seven_zip_found() {
    have 7z || have 7za || [ -f "C:/Program Files/7-Zip/7z.exe" ]
}

check_tools() {
    local found=() tool
    for tool in docker cargo pnpm; do
        if have "$tool"; then found+=("$tool"); else
            case "$tool" in
                docker) row FAIL tools "docker is not installed" "install Docker Desktop (Windows) or Docker Engine with the compose plugin" ;;
                cargo) row FAIL tools "cargo is not installed" "install Rust with rustup (https://rustup.rs)" ;;
                pnpm) row FAIL tools "pnpm is not installed" "corepack enable pnpm, or see https://pnpm.io/installation (never npm)" ;;
            esac
        fi
    done
    if [ -n "$(openssl_cmd)" ]; then found+=(openssl); else
        row FAIL tools "openssl is not installed" "install OpenSSL (Git for Windows ships one); ./cert.sh needs it"
    fi
    if seven_zip_found; then found+=(7z); else
        row WARN tools "7-Zip is not installed" "install it from https://www.7-zip.org/ (Linux: p7zip-full); ./vault.sh needs it"
    fi
    if have "${POSTIT_SQLX:-sqlx}"; then found+=(sqlx); else
        row WARN tools "sqlx-cli is not installed" "cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres --locked"
    fi
    if have flutter; then found+=(flutter); else
        row WARN tools "flutter is not installed" "only the mobile client needs it: Flutter 3.47.6, see mobile/"
    fi
    if have psql; then found+=(psql); else
        row WARN tools "no host psql" "only ./db.sh drop with ADMIN_DATABASE_URL needs it"
    fi
    [ ${#found[@]} -eq 0 ] || row PASS tools "found ${found[*]}"
}

# Every rule of the vault check names the key, never its value.
check_vault() {
    local problems=0 f key dir="$vault" fix_edit="edit !ref/vault/$environment/<file>, then ./vault.sh pack"
    vfail() {
        row FAIL vault "$1" "${2:-$fix_edit}"
        problems=1
    }
    for f in $(vault_files "$environment"); do
        [ -f "$dir/$f" ] || vfail "missing !ref/vault/$environment/$f" "./vault.sh unpack (README: 'Environment files and secrets')"
    done
    if [ "$problems" = 0 ]; then
        local pg="$dir/postgres.env" app="$dir/postit.env"
        for key in POSTGRES_USER POSTGRES_PASSWORD; do
            [ -n "$(env_value "$pg" "$key")" ] || vfail "postgres.env: $key is missing or empty"
        done
        for key in POSTIT__DATABASE__USERNAME POSTIT__DATABASE__PASSWORD POSTIT__AUDIT__PSEUDONYM_KEY; do
            [ -n "$(env_value "$app" "$key")" ] || vfail "postit.env: $key is missing or empty"
        done
        if [ "$environment" = development ]; then
            [ -n "$(env_value "$dir/zitadel.env" ZITADEL_MASTERKEY)" ] ||
                vfail "zitadel.env: ZITADEL_MASTERKEY is missing or empty"
            [ -n "$(env_value "$app" POSTIT__IDENTITY_MAIL__SIGNING_KEY)" ] ||
                vfail "postit.env: POSTIT__IDENTITY_MAIL__SIGNING_KEY is missing or empty" "run cargo xtask zitadel-bootstrap from backend/"
            [ -n "$(env_value "$app" POSTIT__PUSH__VAPID_PRIVATE_KEY)" ] ||
                vfail "postit.env: POSTIT__PUSH__VAPID_PRIVATE_KEY is missing or empty" "run cargo xtask vapid-keygen from backend/ and put the private key in postit.env"
        fi
        local value
        value="$(env_value "$app" POSTIT__AUDIT__PSEUDONYM_KEY)"
        if [ -n "$value" ] && [ ${#value} -lt 32 ]; then
            vfail "postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters"
        fi
        if [ "$environment" != development ]; then
            local names=() name
            while IFS= read -r name; do names+=("$name"); done < <(
                sed -nE 's/^[[:space:]]*(POSTIT__VAULT__KEYS__[1-9][0-9]*)=.*/\1/p' "$app" | sort -u)
            [ ${#names[@]} -gt 0 ] || vfail "postit.env: POSTIT__VAULT__KEYS__1 is missing or empty"
            for name in ${names[@]+"${names[@]}"} POSTIT__MEDIA__SIGNING_KEY POSTIT__EMAIL__SIGNING_KEY; do
                value="$(env_value "$app" "$name")"
                if [ -z "$value" ]; then
                    vfail "postit.env: $name is missing or empty"
                elif [ ${#value} -lt 32 ]; then
                    vfail "postit.env: $name is shorter than 32 characters"
                fi
            done
        fi
        [ "$(env_value "$pg" POSTGRES_USER)" = "$(env_value "$app" POSTIT__DATABASE__USERNAME)" ] ||
            vfail "postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env"
        [ "$(env_value "$pg" POSTGRES_PASSWORD)" = "$(env_value "$app" POSTIT__DATABASE__PASSWORD)" ] ||
            vfail "postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env"
        local smtp_user smtp_pass
        smtp_user="$(env_value "$app" POSTIT__MAIL__SMTP__USERNAME)"
        smtp_pass="$(env_value "$app" POSTIT__MAIL__SMTP__PASSWORD)"
        if [ -n "$smtp_user" ] && [ -z "$smtp_pass" ]; then
            vfail "postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not"
        elif [ -z "$smtp_user" ] && [ -n "$smtp_pass" ]; then
            vfail "postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not"
        fi
    fi
    [ "$problems" = 1 ] || row PASS vault "$environment vault files are complete"
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
check_compose_variables() {
    local key missing=0
    for key in POSTIT_API_URL POSTIT_APP_URL POSTIT_OIDC_ISSUER POSTIT_OIDC_AUDIENCE POSTIT_SMTP_HOST POSTIT_MAIL_FROM; do
        if [ -z "${!key:-}" ] && [ -z "$(env_value "$docker_env_file" "$key")" ]; then
            row FAIL compose-variables "$key is not set in the shell or docker/.env" "set it there (see docker/.env.example)"
            missing=1
        fi
    done
    [ "$missing" = 1 ] || row PASS compose-variables "every variable docker-compose.production.yml requires is set"
}

check_certificate() {
    local f openssl
    for f in postit-dev-ca.crt postit.local.crt postit.local.key; do
        if [ ! -f "$cert_dir/$f" ]; then
            row FAIL certificate "no docker/shared/nginx/certs/$f" "./cert.sh setup"
            return 0
        fi
    done
    openssl="$(openssl_cmd)"
    if [ -n "$openssl" ] && ! "$openssl" x509 -checkend 0 -noout -in "$cert_dir/postit.local.crt" >/dev/null 2>&1; then
        row FAIL certificate "postit.local.crt has expired" "./cert.sh gen"
        return 0
    fi
    row PASS certificate "postit.local certificate and dev CA present"
}

check_certificate_trusted() {
    local ca="$cert_dir/postit-dev-ca.crt"
    if on_windows; then
        local ps
        ps="$(command -v pwsh || command -v powershell || true)"
        if [ -z "$ps" ]; then
            row WARN certificate-trusted "cannot tell without PowerShell" "./cert.sh trust"
            return 0
        fi
        # The CA's thumbprint in the current user's or the machine's Root store.
        if "$ps" -NoProfile -NonInteractive -Command \
            "\$t = [Security.Cryptography.X509Certificates.X509Certificate2]::new('$(native_path "$ca")').Thumbprint; if ((Test-Path \"Cert:\\CurrentUser\\Root\\\$t\") -or (Test-Path \"Cert:\\LocalMachine\\Root\\\$t\")) { exit 0 } else { exit 1 }" \
            >/dev/null 2>&1; then
            row PASS certificate-trusted "the dev CA is in the Windows Root store"
        else
            row WARN certificate-trusted "the dev CA is not trusted; browsers will warn" "./cert.sh trust"
        fi
    elif cmp -s "$ca" /usr/local/share/ca-certificates/postit-dev-ca.crt; then
        row PASS certificate-trusted "the dev CA is in /usr/local/share/ca-certificates"
    else
        row WARN certificate-trusted "the dev CA is not in the system trust store; browsers will warn" "./cert.sh trust"
    fi
}

check_hosts() {
    local hosts=/etc/hosts
    ! on_windows || hosts="${SYSTEMROOT:-C:/Windows}/System32/drivers/etc/hosts"
    if grep -qiE '^[[:space:]]*127\.0\.0\.1[[:space:]]+([^#]*[[:space:]])?postit\.local([[:space:]]|$)' "$hosts" 2>/dev/null; then
        row PASS hosts "postit.local resolves to 127.0.0.1"
    else
        row FAIL hosts "no 127.0.0.1 postit.local entry in $hosts" "add the line '127.0.0.1 postit.local' (needs an administrator)"
    fi
}

check_postgres() {
    if [ "$(docker_cmd inspect --format '{{.State.Running}}' postit-postgres 2>/dev/null || true)" != true ]; then
        row FAIL postgres "postit-postgres is not running" "./compose.sh up"
        return 0
    fi
    if ! docker_cmd exec postit-postgres sh -c 'pg_isready -q -U "$POSTGRES_USER"' >/dev/null 2>&1; then
        row FAIL postgres "postit-postgres is not accepting connections" "wait a moment, or ./compose.sh logs development postit-postgres"
        return 0
    fi
    # The host and port this machine's cargo run and tests use (the vault's DATABASE_URL).
    local url port=5432
    url="$(env_value "$(vault_dir)/development/postgres.env" DATABASE_URL)"
    [[ "$url" =~ @[^:/?]+:([0-9]+) ]] && port="${BASH_REMATCH[1]}"
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
        row PASS postgres "postit-postgres is up and reachable on 127.0.0.1:$port"
    else
        row FAIL postgres "nothing answers on 127.0.0.1:$port, the port in the vault's DATABASE_URL" "./compose.sh ps, and check the port mapping in docker/docker-compose.yml"
    fi
}

check_migration() {
    local applied expected file="$POSTIT_ROOT/backend/crates/data/migrations/0001_init.sql"
    applied="$(docker_cmd exec postit-postgres sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "$1" 2>/dev/null' sh \
        "select encode(checksum, 'hex') from _sqlx_migrations where version = 1" 2>/dev/null | tr -d '\r[:space:]' || true)"
    if [ -z "$applied" ]; then
        row WARN migration "no migration applied yet" "start the server once (cargo dev from backend/, or ./compose.sh up --app)"
        return 0
    fi
    # sqlx records the SHA-384 of the migration file.
    if have sha384sum; then
        expected="$(sha384sum <"$file" | cut -d' ' -f1)"
    else
        expected="$("$(openssl_cmd)" dgst -sha384 -r "$file" | cut -d' ' -f1)"
    fi
    if [ "$applied" = "$expected" ]; then
        row PASS migration "the database matches 0001_init.sql"
    else
        row FAIL migration "the database was built from an older 0001_init.sql" "reset it: ./db.sh drop development, then start the server so it applies the current migration"
    fi
}

check_identity() {
    if ! have curl; then
        row WARN identity "cannot tell without curl" "install curl"
        return 0
    fi
    local status
    status="$(curl -s -o /dev/null -w '%{http_code}' -m 5 --cacert "$(native_path "$cert_dir/postit-dev-ca.crt")" \
        https://postit.local:44300/.well-known/openid-configuration 2>/dev/null || true)"
    if [ "$status" = 200 ]; then
        row PASS identity "Zitadel answers on https://postit.local:44300"
    else
        row FAIL identity "Zitadel's discovery document did not answer (HTTP ${status:-none})" "./compose.sh up, then ./compose.sh logs development postit-zitadel"
    fi
}

check_zitadel_bootstrap() {
    if [ -f "${POSTIT_LOCAL_CONFIG:-$POSTIT_ROOT/backend/config/local.toml}" ]; then
        row PASS zitadel-bootstrap "backend/config/local.toml exists"
    else
        row FAIL zitadel-bootstrap "backend/config/local.toml is missing" "once Zitadel is up: cargo xtask zitadel-bootstrap from backend/"
    fi
}

check_web_packages() {
    if [ -d "${POSTIT_WEB_MODULES:-$POSTIT_ROOT/web/node_modules}" ]; then
        row PASS web-packages "web/node_modules exists"
    else
        row FAIL web-packages "web/node_modules is missing" "pnpm install from web/ (PowerShell on Windows)"
    fi
}

if [ "$environment" = development ]; then
    run tools
    run vault
    run certificate
    run certificate-trusted needs certificate
    run hosts
    run postgres network
    run migration needs postgres network
    run identity needs certificate network
    run zitadel-bootstrap
    run web-packages
else
    run vault
    [ "$environment" != production ] || run compose-variables
fi
exit "$failed"
