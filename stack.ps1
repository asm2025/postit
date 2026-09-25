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
#   build       Build the postit-server image (qa, production)
#   psql        Open a psql shell on the postit database, as the vault's user
#   reset       down -v, then up.  DESTROYS the database
#   help        Show this help message
#
# Environments:
#   development (default), qa, production
#
# Options:
#   -App        development: also run the `app` profile (postit-nginx-app on
#               44300/44305/44310).  Leave it off while `cargo run` / `flutter run` own
#               those ports
#   -Volumes    down: also destroy the database volume
#   -Force      Required to destroy the production database (down -Volumes, reset)
#
# Examples:
#   ./stack.ps1 up                        Development infrastructure, the default
#   ./stack.ps1 up -App                   ...plus the app placeholders
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
$vaultDir = Join-Path $PSScriptRoot "!ref/vault"
$certFile = Join-Path $dockerDir "shared/nginx/certs/postit.local.crt"
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
    $missing = $vaultFiles | Where-Object { -not (Test-Path (Join-Path $vaultDir "$environment/$_")) } |
        ForEach-Object { "!ref/vault/$environment/$_" }
    if ($missing) {
        [Console]::Error.WriteLine("stack: $environment needs these vault files, and they are missing:")
        $missing | ForEach-Object { [Console]::Error.WriteLine("  $_") }
        Stop-Stack "extract !ref/vault.7z first (README: 'Environment files and secrets')"
    }
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
        if ($environment -eq "development") {
            Stop-Stack "development builds nothing; the server runs natively (cargo run) until plan 02 P6"
        }
        Invoke-Docker ($compose + @("build") + $extra)
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
    { $_ -in @("help", "-h", "--help") } { Show-Usage }
    default {
        Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
        Stop-Stack "unknown command: $command"
    }
}
