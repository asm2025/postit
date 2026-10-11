#!/usr/bin/env pwsh
# Launcher for the postit Docker stack.
#
# One command in front of every environment, so the compose -f chain can never be half
# typed: the base file plus exactly one environment file, always. Mirrors stack.sh.
#
# Usage:
#   ./stack.ps1 <command> [environment] [options] [service...]
#
# Commands:
#   up          Bring the stack up in the background.  Pre-flights the environment's
#               vault files (and, in development, the dev certificate) first
#   down        Stop the stack.  -Volumes also DESTROYS the database
#   restart     Restart the stack, or the named services
#   ps          Show container status
#   logs        Follow logs, for the stack or the named services
#   config      Print the resolved compose configuration.  env_file entries stay as
#               paths, so no vault secret reaches the terminal
#   doctor      Check the environment's vault files and required values.  No Docker, no
#               network, and no value is ever printed
#   build       Build the postit-server image (development: add -App)
#   psql        Open a psql shell on the postit database, as the vault's user
#   backup      Dump the postit database to !ref/backups/postit-<environment>-<UTC>.dump
#               (POSTIT_BACKUP_DIR overrides; it must be git-ignored)
#   restore     Restore a dump: ./stack.ps1 restore [environment] <file>.  Refuses while the
#               server runs; production needs -Force.  Does not start anything afterwards
#   reset       down -v, then up.  DESTROYS the database
#   help        Show this help message
#
# Environments:
#   development (default), qa, production
#
# Options:
#   -App        development: also run the `app` profile (postit-server behind
#               postit-nginx-app on 44310/44311/44315).  Leave it off while `cargo run` / the web dev server own
#               those ports
#   -Volumes    down: also destroy the database volume
#   -Force      Required to destroy the production database (down -Volumes, reset)
#
# Examples:
#   ./stack.ps1 up                        Development infrastructure, the default
#   ./stack.ps1 up -App                   ...plus postit-server behind postit-nginx-app
#   ./stack.ps1 logs development postit-zitadel
#   ./stack.ps1 down -Volumes             Development, database destroyed
#   ./stack.ps1 config qa                 What qa resolves to, vault files by path
#   ./stack.ps1 reset production -Force

param(
    [switch]$App,
    [switch]$Volumes,
    [switch]$Force,
    # <command> [environment] [service... | compose flags]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = "Stop"

$dockerDir = Join-Path $PSScriptRoot "docker"
# Overridable for tests (docker/tests/stack-doctor.sh) and unusual layouts.
$vaultDir = if ($env:POSTIT_VAULT_DIR) { $env:POSTIT_VAULT_DIR } else { Join-Path $PSScriptRoot "!ref/vault" }
$dockerEnvFile = if ($env:POSTIT_DOCKER_ENV_FILE) { $env:POSTIT_DOCKER_ENV_FILE } else { Join-Path $dockerDir ".env" }
$backupDir = if ($env:POSTIT_BACKUP_DIR) { $env:POSTIT_BACKUP_DIR } else { Join-Path $PSScriptRoot "!ref/backups" }
$certFile = Join-Path $dockerDir "shared/nginx/certs/postit.local.crt"
$caFile = Join-Path $dockerDir "shared/nginx/certs/postit-dev-ca.crt"
$environments = @("development", "qa", "production")

function Show-Usage {
    # The comment block above, from line 2 to the first blank line.
    $lines = Get-Content $PSCommandPath
    $end = [Array]::IndexOf($lines, "")
    $lines[1..($end - 1)] -replace '^# ?', ''
}

function Stop-Stack([string]$Message) {
    [Console]::Error.WriteLine("stack: $Message")
    exit 1
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { "help" }
$rest = @($Arguments | Select-Object -Skip 1)

$environment = "development"
if ($rest.Count -gt 0 -and $environments -contains $rest[0]) {
    $environment = $rest[0]
    $rest = @($rest | Select-Object -Skip 1)
}

# Anything left — service names, compose flags such as --services or --tail=100 — goes
# to docker compose after the subcommand.
$extra = $rest

if ($App -and $environment -ne "development") {
    Stop-Stack "-App applies to development only; qa and production always run postit-api and postit-worker"
}

$compose = @(
    "compose",
    "-f", (Join-Path $dockerDir "docker-compose.yml"),
    "-f", (Join-Path $dockerDir "docker-compose.$environment.yml")
)

# Every profile, for the commands that must see every container whether or not it was
# started with -App: stopping, listing and following logs.
$composeAll = $compose + @("--profile", "*")

$composeUp = $compose
if ($App) { $composeUp = $compose + @("--profile", "app") }

function Invoke-Docker([string[]]$DockerArgs) {
    & docker @DockerArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# One vault file per project. Compose fails on a missing env_file too, but only for the
# first one it meets and without saying where the file comes from.
$vaultFiles = @("postgres.env", "postit.env")
if ($environment -eq "development") { $vaultFiles += "zitadel.env" }

function Test-Preflight {
    if ($environment -eq "development" -and -not (Test-Path $certFile)) {
        Stop-Stack "no dev certificate at docker/shared/nginx/certs/postit.local.crt — run ./cert.ps1 first"
    }
    if ($environment -eq "development" -and $App -and -not (Test-Path $caFile)) {
        Stop-Stack "no dev CA at docker/shared/nginx/certs/postit-dev-ca.crt — run ./cert.ps1 first"
    }
    $missing = $vaultFiles | Where-Object { -not (Test-Path (Join-Path $vaultDir "$environment/$_")) } |
        ForEach-Object { "!ref/vault/$environment/$_" }
    if ($missing) {
        [Console]::Error.WriteLine("stack: $environment needs these vault files, and they are missing:")
        $missing | ForEach-Object { [Console]::Error.WriteLine("  $_") }
        Stop-Stack "extract !ref/vault.7z first (README: 'Environment files and secrets')"
    }
}

# KEY's value in a dotenv file, read as data (a vault file is never dot-sourced).
# Surrounding single or double quotes are stripped, matching postit-config and Compose.
function Get-EnvValue([string]$File, [string]$Key) {
    if (-not (Test-Path $File)) { return "" }
    $line = Get-Content $File | Where-Object { $_ -cmatch "^\s*$([regex]::Escape($Key))=" } | Select-Object -Last 1
    if ($null -eq $line) { return "" }
    $value = $line.Substring($line.IndexOf('=') + 1)
    if ($value.Length -ge 2 -and (($value[0] -eq "'" -and $value[-1] -eq "'") -or ($value[0] -eq '"' -and $value[-1] -eq '"'))) {
        $value = $value.Substring(1, $value.Length - 2)
    }
    return $value
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
$productionComposeVars = @("POSTIT_API_URL", "POSTIT_APP_URL", "POSTIT_OIDC_ISSUER", "POSTIT_OIDC_AUDIENCE", "POSTIT_SMTP_HOST", "POSTIT_MAIL_FROM")

function Invoke-Doctor {
    $problems = [System.Collections.Generic.List[string]]::new()
    $dir = Join-Path $vaultDir $environment
    foreach ($f in $vaultFiles) {
        if (-not (Test-Path (Join-Path $dir $f))) { $problems.Add("missing !ref/vault/$environment/$f") }
    }
    if ($problems.Count -eq 0) {
        $pg = Join-Path $dir "postgres.env"
        $app = Join-Path $dir "postit.env"
        foreach ($key in @("POSTGRES_USER", "POSTGRES_PASSWORD")) {
            if (-not (Get-EnvValue $pg $key)) { $problems.Add("postgres.env: $key is missing or empty") }
        }
        foreach ($key in @("POSTIT__DATABASE__USERNAME", "POSTIT__DATABASE__PASSWORD", "POSTIT__AUDIT__PSEUDONYM_KEY")) {
            if (-not (Get-EnvValue $app $key)) { $problems.Add("postit.env: $key is missing or empty") }
        }
        if ($environment -eq "development" -and -not (Get-EnvValue (Join-Path $dir "zitadel.env") "ZITADEL_MASTERKEY")) {
            $problems.Add("zitadel.env: ZITADEL_MASTERKEY is missing or empty")
        }
        $pkey = Get-EnvValue $app "POSTIT__AUDIT__PSEUDONYM_KEY"
        if ($pkey -and $pkey.Length -lt 32) {
            $problems.Add("postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters")
        }
        if ((Get-EnvValue $pg "POSTGRES_USER") -cne (Get-EnvValue $app "POSTIT__DATABASE__USERNAME")) {
            $problems.Add("postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env")
        }
        if ((Get-EnvValue $pg "POSTGRES_PASSWORD") -cne (Get-EnvValue $app "POSTIT__DATABASE__PASSWORD")) {
            $problems.Add("postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env")
        }
        $smtpUser = Get-EnvValue $app "POSTIT__MAIL__SMTP__USERNAME"
        $smtpPass = Get-EnvValue $app "POSTIT__MAIL__SMTP__PASSWORD"
        if ($smtpUser -and -not $smtpPass) {
            $problems.Add("postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not")
        } elseif ($smtpPass -and -not $smtpUser) {
            $problems.Add("postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not")
        }
    }
    if ($environment -eq "production") {
        foreach ($key in $productionComposeVars) {
            if (-not [Environment]::GetEnvironmentVariable($key) -and -not (Get-EnvValue $dockerEnvFile $key)) {
                $problems.Add("$key is not set in the shell or docker/.env (see docker/.env.example)")
            }
        }
    }
    if ($problems.Count -gt 0) {
        [Console]::Error.WriteLine("stack: $environment is not ready:")
        $problems | ForEach-Object { [Console]::Error.WriteLine("  $_") }
        exit 1
    }
    Write-Host "stack: $environment looks complete"
    exit 0
}

function Invoke-Backup {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMdd'T'HHmmss'Z'")
    # Absolute, from the shell's location (not .NET's), so git, New-Item and docker cp agree.
    $dir = [IO.Path]::GetFullPath($backupDir, (Get-Location).ProviderPath)
    $file = Join-Path $dir "postit-$environment-$stamp.dump"
    # Checked before anything is created: a refused destination gets no new directory.
    & git -C $PSScriptRoot check-ignore -q $file
    if ($LASTEXITCODE -ne 0) {
        Stop-Stack "refusing to write a dump to ${file}: git does not ignore that path, and a dump is unencrypted user data"
    }
    New-Item -ItemType Directory -Force $dir | Out-Null
    if ($IsWindows) {
        & icacls $dir /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" | Out-Null
    } else {
        & chmod 700 $dir
    }
    $problem = $null
    try {
        # Written inside the container and copied out with docker cp: PowerShell's own
        # redirection would re-encode the binary dump as text.
        & docker exec postit-postgres sh -c 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump'
        if ($LASTEXITCODE -ne 0) {
            $problem = "pg_dump failed"
        } else {
            & docker cp postit-postgres:/tmp/postit.dump $file
            if ($LASTEXITCODE -ne 0) {
                Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
                $problem = "docker cp failed; no dump was kept"
            }
        }
    } finally {
        # The dump is unencrypted user data: never leave it in the container.
        & docker exec postit-postgres sh -c 'rm -f /tmp/postit.dump' 2>&1 | Out-Null
    }
    if ($problem) { Stop-Stack $problem }
    if ($IsWindows) {
        & icacls $file /inheritance:r /grant:r "${env:USERNAME}:F" | Out-Null
    } else {
        & chmod 600 $file
    }
    Write-Host "stack: wrote $file"
    Write-Host "stack: it holds every user's data unencrypted; delete it when you are done with it"
    exit 0
}

function Invoke-Restore {
    $file = if ($extra.Count -gt 0) { $extra[0] } else { "" }
    if (-not $file) { Stop-Stack "usage: ./stack.ps1 restore [environment] <dump file>" }
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { Stop-Stack "no such file: $file" }
    $name = Split-Path $file -Leaf
    if ($name -cnotlike "postit-$environment-*.dump") {
        Stop-Stack "refusing: $name is not a postit-$environment-*.dump file"
    }
    Test-DestroyGuard
    $running = @(& docker ps --format '{{.Names}}' | Where-Object { $_ -match '^postit-(api|worker|server)$' })
    if ($running.Count -gt 0) {
        Stop-Stack "stop the server first (docker stop $($running -join ' ')): it would keep writing to a half-restored database"
    }
    $problem = $null
    try {
        & docker cp $file postit-postgres:/tmp/postit.dump
        if ($LASTEXITCODE -ne 0) {
            $problem = "docker cp failed; nothing was restored"
        } else {
            & docker exec postit-postgres sh -c 'pg_restore --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump'
            if ($LASTEXITCODE -ne 0) {
                $problem = "pg_restore failed; the database may be partly restored. Do not start the server on it"
            }
        }
    } finally {
        & docker exec postit-postgres sh -c 'rm -f /tmp/postit.dump' 2>&1 | Out-Null
    }
    if ($problem) { Stop-Stack $problem }
    Write-Host "stack: restored $file; start the server with ./stack.ps1 up $environment"
    exit 0
}

function Test-DestroyGuard {
    if ($environment -eq "production" -and -not $Force) {
        Stop-Stack "refusing to destroy the production database without -Force"
    }
}

# The cached Zitadel machine key belongs to the Zitadel instance inside the volume. Once
# the volume is gone, `cargo xtask zitadel-bootstrap` would authenticate with a key the
# new instance has never seen, so it goes with the volume.
function Remove-MachineKey {
    if ($environment -ne "development") { return }
    $key = Join-Path $dockerDir "zitadel/machinekey/postit-bootstrap.json"
    if (Test-Path $key) {
        Remove-Item $key
        Write-Host "stack: removed the cached Zitadel machine key; re-run 'cargo xtask zitadel-bootstrap' from server/ once Zitadel is up"
    }
}

switch ($command) {
    "up" {
        Test-Preflight
        Invoke-Docker ($composeUp + @("up", "-d") + $extra)
    }
    "down" {
        if ($Volumes) {
            Test-DestroyGuard
            Invoke-Docker ($composeAll + @("down", "-v") + $extra)
            Remove-MachineKey
        } else {
            Invoke-Docker ($composeAll + @("down") + $extra)
        }
    }
    "restart" { Invoke-Docker ($composeAll + @("restart") + $extra) }
    "ps" { Invoke-Docker ($composeAll + @("ps") + $extra) }
    "logs" { Invoke-Docker ($composeAll + @("logs", "-f") + $extra) }
    "config" { Invoke-Docker ($composeUp + @("config", "--no-env-resolution") + $extra) }
    "build" {
        Invoke-Docker ($composeUp + @("build") + $extra)
    }
    # The container's own POSTGRES_USER (from the vault) over the local socket, which the
    # postgres image trusts — no password on the command line or in the shell.
    "psql" { Invoke-Docker @("exec", "-it", "postit-postgres", "sh", "-c", 'exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"') }
    "reset" {
        Test-DestroyGuard
        Test-Preflight
        Invoke-Docker ($composeAll + @("down", "-v"))
        Remove-MachineKey
        Invoke-Docker ($composeUp + @("up", "-d"))
    }
    "doctor" { Invoke-Doctor }
    "backup" { Invoke-Backup }
    "restore" { Invoke-Restore }
    { $_ -in @("help", "-h", "--help") } { Show-Usage }
    default {
        Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
        Stop-Stack "unknown command: $command"
    }
}
