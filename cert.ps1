#!/usr/bin/env pwsh
# The development TLS certificate: a dev CA and a postit.local leaf for nginx, and trusting
# the CA. Twin: cert.sh.

#Requires -Version 7

param(
    # [setup|gen|trust|help]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'cert'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
cert - generate and trust the development TLS certificate for https://postit.local.

USAGE
  ./cert.ps1 [setup|gen|trust|help]

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
  ./cert.ps1                            First run: generate and trust
  ./cert.ps1 gen                        Renew the leaf, keep the trusted CA

EXIT CODES
  0  success
  1  usage error, or openssl or the trust store failed

MORE
  docs/scripts.md                       every operator script and the first-run order
  README.md, "Hosts file configuration"
'@
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { 'setup' }
if ($Arguments.Count -gt 1) { Stop-Script "unexpected argument: $($Arguments[1])" }
if ($command -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($command -cnotin @('setup', 'gen', 'trust')) {
    Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
    Stop-Script "unknown command: $command"
}

$certDir = if ($env:POSTIT_CERT_DIR) { $env:POSTIT_CERT_DIR } else { "$RepoRoot/docker/shared/nginx/certs" }
$caCrt = 'postit-dev-ca.crt'

function Find-OpenSsl {
    $found = Get-Command openssl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { return $found.Source }
    if (Test-Path -LiteralPath 'C:/Program Files/Git/usr/bin/openssl.exe') { return 'C:/Program Files/Git/usr/bin/openssl.exe' }
    return ''
}

function Invoke-OpenSsl([string[]]$OpenSslArgs) {
    & $script:openssl @OpenSslArgs 2>$null
    if ($LASTEXITCODE -ne 0) { Stop-Script 'openssl failed; see the output above' }
}

function Invoke-Gen {
    $script:openssl = Find-OpenSsl
    if (-not $script:openssl) { Stop-Script 'openssl not found; install OpenSSL (Git for Windows ships one) and re-run' }
    New-Item -ItemType Directory -Force -Path $certDir | Out-Null
    Push-Location -LiteralPath $certDir
    try {
        if (-not (Test-Path -LiteralPath $caCrt)) {
            Invoke-OpenSsl @('req', '-x509', '-newkey', 'rsa:4096', '-sha256', '-days', '3650', '-nodes',
                '-keyout', 'postit-dev-ca.key', '-out', $caCrt, '-subj', '/CN=postit dev CA',
                '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign')
            [Console]::Out.WriteLine("cert: generated the dev CA: $caCrt")
        } else {
            [Console]::Out.WriteLine("cert: reusing the existing dev CA: $caCrt")
        }
        Invoke-OpenSsl @('req', '-newkey', 'rsa:2048', '-sha256', '-nodes', '-keyout', 'postit.local.key', '-out', 'postit.local.csr',
            '-subj', '/CN=postit.local', '-addext', 'subjectAltName=DNS:postit.local,DNS:*.postit.local',
            '-addext', 'extendedKeyUsage=serverAuth')
        Invoke-OpenSsl @('x509', '-req', '-in', 'postit.local.csr', '-CA', $caCrt, '-CAkey', 'postit-dev-ca.key', '-CAcreateserial',
            '-out', 'postit.local.crt', '-days', '825', '-sha256', '-copy_extensions', 'copyall')
        Remove-Item -LiteralPath 'postit.local.csr' -ErrorAction SilentlyContinue
    } finally {
        Pop-Location
    }
    Write-Say "generated the leaf certificate: $certDir/postit.local.crt"
}

function Invoke-Trust {
    $ca = "$certDir/$caCrt"
    if (-not (Test-Path -LiteralPath $ca)) { Stop-Script "no dev CA at $ca; run ./cert.ps1 gen first" }
    if ($IsWindows) {
        Import-Certificate -FilePath $ca -CertStoreLocation Cert:\CurrentUser\Root | Out-Null
        Write-Say "trusted the dev CA in the current user's Windows Root store; restart your browser"
    } elseif ($IsMacOS) {
        & security add-trusted-cert -d -r trustRoot -k "$HOME/Library/Keychains/login.keychain-db" $ca
        if ($LASTEXITCODE -ne 0) { Stop-Script 'could not add the dev CA to the login keychain' }
        Write-Say 'trusted the dev CA in the login keychain; restart your browser'
    } else {
        Write-Say 'trust the dev CA system-wide with:'
        [Console]::Out.WriteLine("  sudo cp `"$ca`" /usr/local/share/ca-certificates/postit-dev-ca.crt")
        [Console]::Out.WriteLine('  sudo update-ca-certificates')
    }
}

function Show-HostsReminder {
    [Console]::Out.WriteLine('')
    [Console]::Out.WriteLine('Add this hosts entry (needs an administrator), then restart your browser:')
    [Console]::Out.WriteLine('  127.0.0.1 postit.local')
}

switch -CaseSensitive ($command) {
    'gen' { Invoke-Gen }
    'trust' { Invoke-Trust }
    'setup' {
        Invoke-Gen
        Invoke-Trust
        Show-HostsReminder
    }
}
exit 0
