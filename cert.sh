#!/usr/bin/env bash
# The development TLS certificate: a dev CA and a postit.local leaf for nginx, and trusting
# the CA. Twin: cert.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=cert
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
cert - generate and trust the development TLS certificate for https://postit.local.

USAGE
  ./cert.sh [setup|gen|trust|help]

COMMANDS
  setup     gen, then trust (the default)
  gen       Write the dev CA (postit-dev-ca.key/.crt, reused when it exists) and a
            fresh postit.local leaf (postit.local.key/.crt, SAN postit.local and
            *.postit.local) into docker/shared/nginx/certs/
  trust     Trust the dev CA: on Windows in the current user's Root store; on
            Linux it prints the sudo commands to run
  help      Show this help (also -h, --help)

OPTIONS
  POSTIT_CERT_DIR   write somewhere other than docker/shared/nginx/certs/

  Then add the hosts entry '127.0.0.1 postit.local' (needs an administrator):
  C:\Windows\System32\drivers\etc\hosts on Windows, /etc/hosts elsewhere.

EXAMPLES
  ./cert.sh                             First run: generate and trust
  ./cert.sh gen                         Renew the leaf, keep the trusted CA

EXIT CODES
  0  success
  1  usage error, or openssl or the trust store failed

MORE
  docs/scripts.md                       every operator script and the first-run order
  README.md, "Hosts file configuration"
EOF
}

command="${1:-setup}"
[ $# -le 1 ] || die "unexpected argument: $2"
case "$command" in
    help | -h | --help)
        usage
        exit 0
        ;;
    setup | gen | trust) ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac

cert_dir="${POSTIT_CERT_DIR:-$POSTIT_ROOT/docker/shared/nginx/certs}"
ca_crt="postit-dev-ca.crt"

openssl_cmd() {
    if command -v openssl >/dev/null 2>&1; then
        printf openssl
    elif [ -f "C:/Program Files/Git/usr/bin/openssl.exe" ]; then
        printf '%s' "C:/Program Files/Git/usr/bin/openssl.exe"
    fi
}

gen() {
    local openssl
    openssl="$(openssl_cmd)"
    [ -n "$openssl" ] || die "openssl not found; install OpenSSL (Git for Windows ships one) and re-run"
    mkdir -p "$cert_dir"
    (
        cd "$cert_dir"
        if [ ! -f "$ca_crt" ]; then
            "$openssl" req -x509 -newkey rsa:4096 -sha256 -days 3650 -nodes \
                -keyout postit-dev-ca.key -out "$ca_crt" \
                -subj "/CN=postit dev CA" \
                -addext "basicConstraints=critical,CA:TRUE" \
                -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
            echo "cert: generated the dev CA: $ca_crt"
        else
            echo "cert: reusing the existing dev CA: $ca_crt"
        fi
        "$openssl" req -newkey rsa:2048 -sha256 -nodes -keyout postit.local.key -out postit.local.csr \
            -subj "/CN=postit.local" -addext "subjectAltName=DNS:postit.local,DNS:*.postit.local" \
            -addext "extendedKeyUsage=serverAuth" 2>/dev/null
        "$openssl" x509 -req -in postit.local.csr -CA "$ca_crt" -CAkey postit-dev-ca.key -CAcreateserial \
            -out postit.local.crt -days 825 -sha256 -copy_extensions copyall 2>/dev/null
        rm -f postit.local.csr
    ) || die "openssl failed; see the output above"
    say "generated the leaf certificate: $cert_dir/postit.local.crt"
}

trust() {
    local ca="$cert_dir/$ca_crt"
    [ -f "$ca" ] || die "no dev CA at $ca; run ./cert.sh gen first"
    if [ -n "${SYSTEMROOT:-}" ] || [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* ]]; then
        local ps
        ps="$(command -v pwsh || command -v powershell || true)"
        [ -n "$ps" ] || die "PowerShell not found; run ./cert.ps1 trust instead"
        "$ps" -NoProfile -NonInteractive -Command \
            "Import-Certificate -FilePath '$(native_path "$ca")' -CertStoreLocation Cert:\\CurrentUser\\Root | Out-Null" ||
            die "could not add the dev CA to the current user's Root store"
        say "trusted the dev CA in the current user's Windows Root store; restart your browser"
    elif [ "$(uname -s)" = Darwin ]; then
        security add-trusted-cert -d -r trustRoot -k "$HOME/Library/Keychains/login.keychain-db" "$ca" ||
            die "could not add the dev CA to the login keychain"
        say "trusted the dev CA in the login keychain; restart your browser"
    else
        say "trust the dev CA system-wide with:"
        echo "  sudo cp \"$ca\" /usr/local/share/ca-certificates/postit-dev-ca.crt"
        echo "  sudo update-ca-certificates"
    fi
}

hosts_reminder() {
    echo ""
    echo "Add this hosts entry (needs an administrator), then restart your browser:"
    echo "  127.0.0.1 postit.local"
}

case "$command" in
    gen) gen ;;
    trust) trust ;;
    setup)
        gen
        trust
        hosts_reminder
        ;;
esac
