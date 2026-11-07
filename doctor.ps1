#!/usr/bin/env pwsh
# Is this machine ready, and is an environment's vault complete? Read-only. Twin: doctor.sh.

#Requires -Version 7

param(
    # [check] [environment]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'doctor'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
doctor - check that this machine is ready and an environment's vault is complete.

USAGE
  ./doctor.ps1 [check] [environment]

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
  ./doctor.ps1                          Is this machine ready for development?
  ./doctor.ps1 check qa                 Is the qa vault complete?
  $env:POSTIT_DOCTOR_ONLY = 'migration'; ./doctor.ps1

EXIT CODES
  0  no FAIL row (WARN rows allowed)
  1  at least one FAIL row, or a usage error

MORE
  docs/scripts.md                       every operator script and the first-run order
  README.md, "Environment files and secrets"
'@
}

$rest = @($Arguments)
if ($rest.Count -gt 0 -and $rest[0] -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($rest.Count -gt 0 -and $rest[0] -ceq 'check') { $rest = @($rest | Select-Object -Skip 1) }
$environment = 'development'
if ($rest.Count -gt 0) {
    $environment = Test-Environment $rest[0]
    if (-not $environment) {
        Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
        Stop-Script "unknown command or environment: $($rest[0])"
    }
    $rest = @($rest | Select-Object -Skip 1)
}
if ($rest.Count -gt 0) { Stop-Script "unexpected argument: $($rest[0])" }

$vault = "$VaultDir/$environment"
$certDir = if ($env:POSTIT_CERT_DIR) { $env:POSTIT_CERT_DIR } else { "$RepoRoot/docker/shared/nginx/certs" }
$dockerEnvFile = if ($env:POSTIT_DOCKER_ENV_FILE) { $env:POSTIT_DOCKER_ENV_FILE } else { "$RepoRoot/docker/.env" }
$only = @("$env:POSTIT_DOCTOR_ONLY" -split '[,\s]+' | Where-Object { $_ })
$offline = [bool]$env:POSTIT_DOCTOR_OFFLINE
$script:failed = $false
$script:result = @{}

function Write-Row([string]$Status, [string]$Check, [string]$Finding, [string]$Fix = '') {
    $line = "$Status  $Check  $Finding"
    if ($Fix) { $line += " -> $Fix" }
    [Console]::Out.WriteLine($line)
    $current = $script:result[$Check]
    switch ($Status) {
        'FAIL' { $script:failed = $true; $script:result[$Check] = 'FAIL' }
        'WARN' { if ($current -ne 'FAIL') { $script:result[$Check] = 'WARN' } }
        default { if (-not $current) { $script:result[$Check] = $Status } }
    }
}

# Runs the check unless POSTIT_DOCTOR_ONLY leaves it out, a prerequisite that ran did not
# pass, or it needs Docker or the network while offline.
function Invoke-Check([string]$Name, [string]$Needs = '', [switch]$Network) {
    if ($only.Count -gt 0 -and $only -cnotcontains $Name) { return }
    if ($Network -and $offline) { Write-Row 'SKIP' $Name '(offline)'; return }
    if ($Needs -and $script:result[$Needs] -and $script:result[$Needs] -notin @('PASS', 'WARN')) {
        Write-Row 'SKIP' $Name "(needs $Needs)"
        return
    }
    & "Test-$(($Name -split '-' | ForEach-Object { $_.Substring(0, 1).ToUpper() + $_.Substring(1) }) -join '')"
}

function Test-Have([string]$Name) { return [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Get-OpenSsl {
    $found = Get-Command openssl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { return $found.Source }
    if (Test-Path -LiteralPath 'C:/Program Files/Git/usr/bin/openssl.exe') { return 'C:/Program Files/Git/usr/bin/openssl.exe' }
    return ''
}

function Test-Tools {
    $found = @()
    foreach ($tool in @('docker', 'cargo', 'pnpm')) {
        if (Test-Have $tool) { $found += $tool; continue }
        switch ($tool) {
            'docker' { Write-Row 'FAIL' 'tools' 'docker is not installed' 'install Docker Desktop (Windows) or Docker Engine with the compose plugin' }
            'cargo' { Write-Row 'FAIL' 'tools' 'cargo is not installed' 'install Rust with rustup (https://rustup.rs)' }
            'pnpm' { Write-Row 'FAIL' 'tools' 'pnpm is not installed' 'corepack enable pnpm, or see https://pnpm.io/installation (never npm)' }
        }
    }
    if (Get-OpenSsl) { $found += 'openssl' } else {
        Write-Row 'FAIL' 'tools' 'openssl is not installed' 'install OpenSSL (Git for Windows ships one); ./cert.ps1 needs it'
    }
    if ((Test-Have '7z') -or (Test-Have '7za') -or (Test-Path -LiteralPath 'C:/Program Files/7-Zip/7z.exe')) { $found += '7z' } else {
        Write-Row 'WARN' 'tools' '7-Zip is not installed' 'install it from https://www.7-zip.org/ (Linux: p7zip-full); ./vault.ps1 needs it'
    }
    $sqlx = if ($env:POSTIT_SQLX) { $env:POSTIT_SQLX } else { 'sqlx' }
    if (Test-Have $sqlx) { $found += 'sqlx' } else {
        Write-Row 'WARN' 'tools' 'sqlx-cli is not installed' 'cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres --locked'
    }
    if (Test-Have 'flutter') { $found += 'flutter' } else {
        Write-Row 'WARN' 'tools' 'flutter is not installed' 'only the mobile client needs it: Flutter 3.47.6, see mobile/'
    }
    if (Test-Have 'psql') { $found += 'psql' } else {
        Write-Row 'WARN' 'tools' 'no host psql' 'only ./db.ps1 drop with ADMIN_DATABASE_URL needs it'
    }
    if ($found.Count -gt 0) { Write-Row 'PASS' 'tools' "found $($found -join ' ')" }
}

# Every rule of the vault check names the key, never its value.
function Test-Vault {
    $script:vaultProblems = $false
    $fixEdit = "edit !ref/vault/$environment/<file>, then ./vault.ps1 pack"
    function Fail([string]$Finding, [string]$Fix = $fixEdit) {
        Write-Row 'FAIL' 'vault' $Finding $Fix
        $script:vaultProblems = $true
    }
    foreach ($f in (Get-VaultFiles $environment)) {
        if (-not (Test-Path -LiteralPath "$vault/$f" -PathType Leaf)) {
            Fail "missing !ref/vault/$environment/$f" "./vault.ps1 unpack (README: 'Environment files and secrets')"
        }
    }
    if (-not $script:vaultProblems) {
        $pg = "$vault/postgres.env"
        $app = "$vault/postit.env"
        foreach ($key in @('POSTGRES_USER', 'POSTGRES_PASSWORD')) {
            if (-not (Get-EnvValue $pg $key)) { Fail "postgres.env: $key is missing or empty" }
        }
        foreach ($key in @('POSTIT__DATABASE__USERNAME', 'POSTIT__DATABASE__PASSWORD', 'POSTIT__AUDIT__PSEUDONYM_KEY')) {
            if (-not (Get-EnvValue $app $key)) { Fail "postit.env: $key is missing or empty" }
        }
        if ($environment -eq 'development') {
            if (-not (Get-EnvValue "$vault/zitadel.env" 'ZITADEL_MASTERKEY')) { Fail 'zitadel.env: ZITADEL_MASTERKEY is missing or empty' }
            if (-not (Get-EnvValue $app 'POSTIT__IDENTITY_MAIL__SIGNING_KEY')) {
                Fail 'postit.env: POSTIT__IDENTITY_MAIL__SIGNING_KEY is missing or empty' 'run cargo xtask zitadel-bootstrap from backend/'
            }
            if (-not (Get-EnvValue $app 'POSTIT__PUSH__VAPID_PRIVATE_KEY')) {
                Fail 'postit.env: POSTIT__PUSH__VAPID_PRIVATE_KEY is missing or empty' 'run cargo xtask vapid-keygen from backend/ and put the private key in postit.env'
            }
        }
        $value = Get-EnvValue $app 'POSTIT__AUDIT__PSEUDONYM_KEY'
        if ($value -and $value.Length -lt 32) { Fail 'postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters' }
        if ($environment -ne 'development') {
            $names = @(Get-Content -LiteralPath $app | ForEach-Object { if ($_ -cmatch '^\s*(POSTIT__VAULT__KEYS__[1-9][0-9]*)=') { $Matches[1] } } |
                    Sort-Object -Unique -CaseSensitive)
            if ($names.Count -eq 0) { Fail 'postit.env: POSTIT__VAULT__KEYS__1 is missing or empty' }
            foreach ($name in ($names + @('POSTIT__MEDIA__SIGNING_KEY', 'POSTIT__EMAIL__SIGNING_KEY'))) {
                $value = Get-EnvValue $app $name
                if (-not $value) { Fail "postit.env: $name is missing or empty" }
                elseif ($value.Length -lt 32) { Fail "postit.env: $name is shorter than 32 characters" }
            }
        }
        if ((Get-EnvValue $pg 'POSTGRES_USER') -cne (Get-EnvValue $app 'POSTIT__DATABASE__USERNAME')) {
            Fail 'postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env'
        }
        if ((Get-EnvValue $pg 'POSTGRES_PASSWORD') -cne (Get-EnvValue $app 'POSTIT__DATABASE__PASSWORD')) {
            Fail 'postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env'
        }
        $smtpUser = Get-EnvValue $app 'POSTIT__MAIL__SMTP__USERNAME'
        $smtpPass = Get-EnvValue $app 'POSTIT__MAIL__SMTP__PASSWORD'
        if ($smtpUser -and -not $smtpPass) {
            Fail 'postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not'
        } elseif ($smtpPass -and -not $smtpUser) {
            Fail 'postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not'
        }
    }
    if (-not $script:vaultProblems) { Write-Row 'PASS' 'vault' "$environment vault files are complete" }
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
function Test-ComposeVariables {
    $missing = $false
    foreach ($key in @('POSTIT_API_URL', 'POSTIT_APP_URL', 'POSTIT_OIDC_ISSUER', 'POSTIT_OIDC_AUDIENCE', 'POSTIT_SMTP_HOST', 'POSTIT_MAIL_FROM')) {
        if (-not [Environment]::GetEnvironmentVariable($key) -and -not (Get-EnvValue $dockerEnvFile $key)) {
            Write-Row 'FAIL' 'compose-variables' "$key is not set in the shell or docker/.env" 'set it there (see docker/.env.example)'
            $missing = $true
        }
    }
    if (-not $missing) { Write-Row 'PASS' 'compose-variables' 'every variable docker-compose.production.yml requires is set' }
}

function Test-Certificate {
    foreach ($f in @('postit-dev-ca.crt', 'postit.local.crt', 'postit.local.key')) {
        if (-not (Test-Path -LiteralPath "$certDir/$f" -PathType Leaf)) {
            Write-Row 'FAIL' 'certificate' "no docker/shared/nginx/certs/$f" './cert.ps1 setup'
            return
        }
    }
    $openssl = Get-OpenSsl
    if ($openssl) {
        & $openssl x509 -checkend 0 -noout -in "$certDir/postit.local.crt" *> $null
        if ($LASTEXITCODE -ne 0) {
            Write-Row 'FAIL' 'certificate' 'postit.local.crt has expired' './cert.ps1 gen'
            return
        }
    }
    Write-Row 'PASS' 'certificate' 'postit.local certificate and dev CA present'
}

function Test-CertificateTrusted {
    $ca = "$certDir/postit-dev-ca.crt"
    if ($IsWindows) {
        # The CA's thumbprint in the current user's or the machine's Root store.
        $thumb = [Security.Cryptography.X509Certificates.X509Certificate2]::new($ca).Thumbprint
        if ((Test-Path "Cert:\CurrentUser\Root\$thumb") -or (Test-Path "Cert:\LocalMachine\Root\$thumb")) {
            Write-Row 'PASS' 'certificate-trusted' 'the dev CA is in the Windows Root store'
        } else {
            Write-Row 'WARN' 'certificate-trusted' 'the dev CA is not trusted; browsers will warn' './cert.ps1 trust'
        }
        return
    }
    $installed = '/usr/local/share/ca-certificates/postit-dev-ca.crt'
    if ((Test-Path -LiteralPath $installed) -and (Get-FileHash -LiteralPath $installed).Hash -eq (Get-FileHash -LiteralPath $ca).Hash) {
        Write-Row 'PASS' 'certificate-trusted' 'the dev CA is in /usr/local/share/ca-certificates'
    } else {
        Write-Row 'WARN' 'certificate-trusted' 'the dev CA is not in the system trust store; browsers will warn' './cert.ps1 trust'
    }
}

function Test-Hosts {
    $hosts = if ($IsWindows) { "$(if ($env:SYSTEMROOT) { $env:SYSTEMROOT } else { 'C:/Windows' })/System32/drivers/etc/hosts" -replace '\\', '/' } else { '/etc/hosts' }
    $entry = (Test-Path -LiteralPath $hosts) -and
        (Get-Content -LiteralPath $hosts | Where-Object { $_ -match '^\s*127\.0\.0\.1\s+([^#]*\s)?postit\.local(\s|$)' })
    if ($entry) {
        Write-Row 'PASS' 'hosts' 'postit.local resolves to 127.0.0.1'
    } else {
        Write-Row 'FAIL' 'hosts' "no 127.0.0.1 postit.local entry in $hosts" "add the line '127.0.0.1 postit.local' (needs an administrator)"
    }
}

function Test-Postgres {
    if (-not (Test-Have $Docker)) {
        Write-Row 'FAIL' 'postgres' 'docker is not available' "install Docker Desktop (Windows) or Docker Engine with the compose plugin, then ./compose.ps1 up"
        return
    }
    $state = & $Docker inspect --format '{{.State.Running}}' postit-postgres 2>$null
    if ($LASTEXITCODE -ne 0 -or "$state".Trim() -ne 'true') {
        Write-Row 'FAIL' 'postgres' 'postit-postgres is not running' './compose.ps1 up'
        return
    }
    & $Docker exec postit-postgres sh -c 'pg_isready -q -U "$POSTGRES_USER"' *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Row 'FAIL' 'postgres' 'postit-postgres is not accepting connections' 'wait a moment, or ./compose.ps1 logs development postit-postgres'
        return
    }
    # The host and port this machine's cargo run and tests use (the vault's DATABASE_URL).
    $url = Get-EnvValue "$VaultDir/development/postgres.env" 'DATABASE_URL'
    $port = if ($url -match '@[^:/?]+:(\d+)') { [int]$Matches[1] } else { 5432 }
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $open = $client.ConnectAsync('127.0.0.1', $port).Wait(3000) -and $client.Connected
    } catch {
        $open = $false
    } finally {
        $client.Dispose()
    }
    if ($open) {
        Write-Row 'PASS' 'postgres' "postit-postgres is up and reachable on 127.0.0.1:$port"
    } else {
        Write-Row 'FAIL' 'postgres' "nothing answers on 127.0.0.1:$port, the port in the vault's DATABASE_URL" './compose.ps1 ps, and check the port mapping in docker/docker-compose.yml'
    }
}

function Test-Migration {
    $file = "$RepoRoot/backend/crates/data/migrations/0001_init.sql"
    if (-not (Test-Have $Docker)) {
        Write-Row 'FAIL' 'migration' 'docker is not available' "install Docker Desktop (Windows) or Docker Engine with the compose plugin, then ./compose.ps1 up"
        return
    }
    $applied = & $Docker exec postit-postgres sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "$1" 2>/dev/null' sh "select encode(checksum, 'hex') from _sqlx_migrations where version = 1" 2>$null
    $applied = ("$applied" -replace '\s', '')
    if (-not $applied) {
        Write-Row 'WARN' 'migration' 'no migration applied yet' 'start the server once (cargo dev from backend/, or ./compose.ps1 up -App)'
        return
    }
    # sqlx records the SHA-384 of the migration file.
    $expected = (Get-FileHash -LiteralPath $file -Algorithm SHA384).Hash.ToLowerInvariant()
    if ($applied -ceq $expected) {
        Write-Row 'PASS' 'migration' 'the database matches 0001_init.sql'
    } else {
        Write-Row 'FAIL' 'migration' 'the database was built from an older 0001_init.sql' 'reset it: ./db.ps1 drop development, then start the server so it applies the current migration'
    }
}

function Test-Identity {
    $curl = if ($env:POSTIT_CURL) { $env:POSTIT_CURL } else {
        (Get-Command curl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    }
    if (-not $curl -or -not (Test-Have $curl)) {
        Write-Row 'WARN' 'identity' 'cannot tell without curl' 'install curl'
        return
    }
    $discard = if ($IsWindows) { 'NUL' } else { '/dev/null' }
    $curlArgs = @('-s', '-o', $discard, '-w', '%{http_code}', '-m', '5')
    # Windows curl (schannel) cannot check revocation for the dev CA, which has no CRL.
    if ($IsWindows) { $curlArgs += '--ssl-no-revoke' }
    $curlArgs += @('--cacert', "$certDir/postit-dev-ca.crt", 'https://postit.local:44300/.well-known/openid-configuration')
    $status = & $curl @curlArgs 2>$null
    if ("$status" -eq '200') {
        Write-Row 'PASS' 'identity' 'Zitadel answers on https://postit.local:44300'
    } elseif ("$status" -eq '502') {
        # nginx resolves postit-zitadel once, at its own start.
        Write-Row 'FAIL' 'identity' 'postit-nginx-infra cannot reach Zitadel (HTTP 502)' './compose.ps1 restart development postit-nginx-infra, after Zitadel was recreated'
    } else {
        $shown = if ($status) { $status } else { 'none' }
        Write-Row 'FAIL' 'identity' "Zitadel's discovery document did not answer (HTTP $shown)" './compose.ps1 up, then ./compose.ps1 logs development postit-zitadel'
    }
}

function Test-ZitadelBootstrap {
    $path = if ($env:POSTIT_LOCAL_CONFIG) { $env:POSTIT_LOCAL_CONFIG } else { "$RepoRoot/backend/config/local.toml" }
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        Write-Row 'PASS' 'zitadel-bootstrap' 'backend/config/local.toml exists'
    } else {
        Write-Row 'FAIL' 'zitadel-bootstrap' 'backend/config/local.toml is missing' 'once Zitadel is up: cargo xtask zitadel-bootstrap from backend/'
    }
}

function Test-WebPackages {
    $path = if ($env:POSTIT_WEB_MODULES) { $env:POSTIT_WEB_MODULES } else { "$RepoRoot/web/node_modules" }
    if (Test-Path -LiteralPath $path -PathType Container) {
        Write-Row 'PASS' 'web-packages' 'web/node_modules exists'
    } else {
        Write-Row 'FAIL' 'web-packages' 'web/node_modules is missing' 'pnpm install from web/ (PowerShell on Windows)'
    }
}

if ($environment -eq 'development') {
    Invoke-Check 'tools'
    Invoke-Check 'vault'
    Invoke-Check 'certificate'
    Invoke-Check 'certificate-trusted' -Needs 'certificate'
    Invoke-Check 'hosts'
    Invoke-Check 'postgres' -Network
    Invoke-Check 'migration' -Needs 'postgres' -Network
    Invoke-Check 'identity' -Needs 'certificate' -Network
    Invoke-Check 'zitadel-bootstrap'
    Invoke-Check 'web-packages'
} else {
    Invoke-Check 'vault'
    if ($environment -eq 'production') { Invoke-Check 'compose-variables' }
}
exit ([int]$script:failed)
