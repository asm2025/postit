#!/usr/bin/env bash
# Generates a dev CA and a postly.local leaf certificate for nginx TLS termination
# (docker/shared/nginx/certs), and trusts the CA in the OS trust store (macOS) or prints
# the command to do so (Linux, needs sudo). Requires openssl.
#
# Usage: ./cert.sh
# Then add this to /etc/hosts:
#   127.0.0.1 postly.local

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cert_dir="${1:-$script_dir/docker/shared/nginx/certs}"
mkdir -p "$cert_dir"
cd "$cert_dir"

ca_key="postly-dev-ca.key"
ca_crt="postly-dev-ca.crt"
leaf_key="postly.local.key"
leaf_crt="postly.local.crt"
leaf_csr="postly.local.csr"
san="subjectAltName=DNS:postly.local,DNS:*.postly.local"

if [ ! -f "$ca_crt" ]; then
    openssl req -x509 -newkey rsa:4096 -sha256 -days 3650 -nodes \
        -keyout "$ca_key" -out "$ca_crt" \
        -subj "/CN=postly dev CA" \
        -addext "basicConstraints=critical,CA:TRUE" \
        -addext "keyUsage=critical,keyCertSign,cRLSign"
    echo "Generated dev CA: $ca_crt"
else
    echo "Reusing existing dev CA: $ca_crt"
fi

openssl req -newkey rsa:2048 -sha256 -nodes -keyout "$leaf_key" -out "$leaf_csr" \
    -subj "/CN=postly.local" -addext "$san" -addext "extendedKeyUsage=serverAuth"
openssl x509 -req -in "$leaf_csr" -CA "$ca_crt" -CAkey "$ca_key" -CAcreateserial \
    -out "$leaf_crt" -days 825 -sha256 -copy_extensions copyall
rm -f "$leaf_csr"

echo "Generated leaf certificate: $leaf_crt"

case "$(uname -s)" in
    Darwin)
        echo "Trusting dev CA in the login keychain..."
        security add-trusted-cert -d -r trustRoot -k "$HOME/Library/Keychains/login.keychain-db" "$ca_crt"
        ;;
    Linux)
        echo ""
        echo "Trust the dev CA system-wide with:"
        echo "  sudo cp \"$ca_crt\" /usr/local/share/ca-certificates/postly-dev-ca.crt"
        echo "  sudo update-ca-certificates"
        ;;
    *)
        echo "Unrecognized OS; trust $ca_crt manually."
        ;;
esac

echo ""
echo "Done. Add this hosts entry (needs sudo), then restart your browser:"
echo "  127.0.0.1 postly.local"
echo "  File: /etc/hosts"
