#!/usr/bin/env pwsh
# Generates a dev CA and a postly.local leaf certificate for nginx TLS termination
# (docker/shared/nginx/certs), and trusts the CA in the current Windows user's Root
# store. Requires openssl (ships with Git for Windows at usr\bin\openssl.exe).
#
# Usage: ./cert.ps1
# Then add this to C:\Windows\System32\drivers\etc\hosts (needs an elevated editor):
#   127.0.0.1 postly.local

param(
    [string]$CertDir = (Join-Path $PSScriptRoot "docker/shared/nginx/certs")
)

$ErrorActionPreference = "Stop"

function Find-OpenSsl {
    $cmd = Get-Command openssl -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $gitOpenSsl = "C:\Program Files\Git\usr\bin\openssl.exe"
    if (Test-Path $gitOpenSsl) { return $gitOpenSsl }
    throw "openssl not found. Install Git for Windows (bundles openssl) or OpenSSL directly, then re-run."
}

$openssl = Find-OpenSsl
New-Item -ItemType Directory -Force -Path $CertDir | Out-Null
Set-Location -Path $CertDir

$caKey = "postly-dev-ca.key"
$caCrt = "postly-dev-ca.crt"
$leafKey = "postly.local.key"
$leafCrt = "postly.local.crt"
$leafCsr = "postly.local.csr"
$san = "subjectAltName=DNS:postly.local,DNS:*.postly.local"

if (-not (Test-Path $caCrt)) {
    & $openssl req -x509 -newkey rsa:4096 -sha256 -days 3650 -nodes `
        -keyout $caKey -out $caCrt -subj "/CN=postly dev CA" `
        -addext "basicConstraints=critical,CA:TRUE" `
        -addext "keyUsage=critical,keyCertSign,cRLSign"
    if ($LASTEXITCODE -ne 0) { throw "openssl failed generating the dev CA" }
    Write-Host "Generated dev CA: $caCrt"
} else {
    Write-Host "Reusing existing dev CA: $caCrt"
}

& $openssl req -newkey rsa:2048 -sha256 -nodes -keyout $leafKey -out $leafCsr `
    -subj "/CN=postly.local" -addext $san -addext "extendedKeyUsage=serverAuth"
if ($LASTEXITCODE -ne 0) { throw "openssl failed generating the leaf CSR" }

& $openssl x509 -req -in $leafCsr -CA $caCrt -CAkey $caKey -CAcreateserial `
    -out $leafCrt -days 825 -sha256 -copy_extensions copyall
if ($LASTEXITCODE -ne 0) { throw "openssl failed signing the leaf certificate" }

Remove-Item $leafCsr -ErrorAction SilentlyContinue

Write-Host "Generated leaf certificate: $leafCrt"

Write-Host "Trusting dev CA in the current user's Windows Root store..."
Import-Certificate -FilePath $caCrt -CertStoreLocation Cert:\CurrentUser\Root | Out-Null

Write-Host ""
Write-Host "Done. Add this hosts entry as Administrator, then restart your browser:"
Write-Host "  127.0.0.1 postly.local"
Write-Host "  File: C:\Windows\System32\drivers\etc\hosts"
