#!/usr/bin/env pwsh
# The postit Docker Compose stack, one environment at a time. Twin: compose.sh.

#Requires -Version 7

param(
    [switch]$App,
    [switch]$Volumes,
    [switch]$Force,
    # <command> [environment] [service... | compose flags]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'compose'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
compose - run the postit Docker Compose stack for one environment.

USAGE
  ./compose.ps1 <command> [environment] [options] [docker compose arguments]

COMMANDS
  up        Check the environment's vault files (and, in development, the dev
            certificate), then start the stack in the background
  down      Stop and remove the containers; -Volumes also deletes the database
  restart   Restart the stack, or the named services
  ps        Show container status
  logs      Follow the logs of the stack, or of the named services
  config    Print the resolved configuration; vault files stay paths, so no secret
            is printed
  build     Build the postit-server image (development: add -App)
  help      Show this help (also -h, --help, or no command)

OPTIONS
  environment    development (the default), qa, or production
  -App           development only: also run the app profile (postit-server behind
                 postit-nginx-app on 44310, 44311, 44315). Leave it off while a
                 native cargo run or the web dev server owns those ports
  -Volumes       down: also delete the database volume. You type the environment
                 name to confirm; in development the cached Zitadel machine key goes
                 too, so re-run cargo xtask zitadel-bootstrap afterwards
  -Force         down -Volumes: skip the question in development and qa.
                 Production requires -Force and still asks
  Anything else goes to docker compose after the command (service names,
  --tail=100, --services).

EXAMPLES
  ./compose.ps1 up                      Development infrastructure
  ./compose.ps1 up -App                 ...plus postit-server behind postit-nginx-app
  ./compose.ps1 logs development postit-zitadel --tail=100
  ./compose.ps1 config qa               What qa resolves to, vault files by path
  ./compose.ps1 down -Volumes           Delete the development database; then
  ./compose.ps1 up                      start again on an empty one

EXIT CODES
  0  success
  1  usage error, a refused guard, or an unconfirmed question
  *  any other code is docker compose's own

MORE
  docs/scripts.md                       every operator script and the first-run order
  docs/contracts/04. infrastructure and tooling.md
'@
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { 'help' }
$rest = @($Arguments | Select-Object -Skip 1)

if ($command -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($command -cnotin @('up', 'down', 'restart', 'ps', 'logs', 'config', 'build')) {
    Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
    Stop-Script "unknown command: $command"
}

$environment = 'development'
if ($rest.Count -gt 0 -and (Test-Environment $rest[0])) {
    $environment = $rest[0]
    $rest = @($rest | Select-Object -Skip 1)
}
# Anything left (service names, compose flags such as --tail=100) goes to docker compose
# after the subcommand.
$extra = $rest

if ($App -and $environment -ne 'development') {
    Stop-Script '-App applies to development only; qa and production always run postit-api and postit-worker'
}

$compose = Get-ComposeArgs $environment
# Every profile, for the commands that must see every container whether or not it was
# started with -App: stopping, listing, restarting, and following logs.
$composeAll = $compose + @('--profile', '*')
$composeUp = if ($App) { $compose + @('--profile', 'app') } else { $compose }

$certDir = if ($env:POSTIT_CERT_DIR) { $env:POSTIT_CERT_DIR } else { "$RepoRoot/docker/shared/nginx/certs" }

# Compose fails on a missing env_file too, but only for the first one it meets and
# without saying where the file comes from.
function Test-Preflight {
    if ($environment -eq 'development' -and -not (Test-Path -LiteralPath "$certDir/postit.local.crt")) {
        Stop-Script 'no dev certificate at docker/shared/nginx/certs/postit.local.crt; run ./cert.ps1 first'
    }
    if ($environment -eq 'development' -and $App -and -not (Test-Path -LiteralPath "$certDir/postit-dev-ca.crt")) {
        Stop-Script 'no dev CA at docker/shared/nginx/certs/postit-dev-ca.crt; run ./cert.ps1 first'
    }
    $missing = @(Get-VaultFiles $environment | Where-Object { -not (Test-Path -LiteralPath "$VaultDir/$environment/$_") } |
            ForEach-Object { "!ref/vault/$environment/$_" })
    if ($missing.Count -gt 0) {
        [Console]::Error.WriteLine("compose: $environment needs these vault files, and they are missing:")
        $missing | ForEach-Object { [Console]::Error.WriteLine("  $_") }
        Stop-Script "run ./vault.ps1 unpack first (README: 'Environment files and secrets')"
    }
}

# Deleting the database volume: development and qa ask unless -Force; production
# requires -Force and asks anyway.
function Confirm-Volumes {
    if ($environment -eq 'production' -and -not $Force) {
        Stop-Script 'refusing to delete the production database volume without -Force'
    }
    $running = Get-RunningEnvironment
    if ($running -and $running -ne $environment) {
        Stop-Script "you asked for $environment, but the running postit-postgres belongs to $running; refusing to delete its volume under $environment's rules"
    }
    if ($environment -eq 'production' -or -not $Force) {
        if (-not (Confirm-Typed $environment "Type the environment name ($environment) to delete its database volume: ")) {
            Stop-Script 'confirmation did not match; nothing was stopped or deleted'
        }
    }
}

# The cached Zitadel machine key belongs to the Zitadel instance inside the volume. Once
# the volume is gone, `cargo xtask zitadel-bootstrap` would authenticate with a key the
# new instance has never seen, so it goes with the volume.
function Remove-MachineKey {
    if ($environment -ne 'development') { return }
    $key = if ($env:POSTIT_MACHINE_KEY) { $env:POSTIT_MACHINE_KEY } else { "$RepoRoot/docker/zitadel/machinekey/postit-bootstrap.json" }
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key
        Write-Say "removed the cached Zitadel machine key; re-run 'cargo xtask zitadel-bootstrap' from backend/ once Zitadel is up"
    }
}

switch -CaseSensitive ($command) {
    'up' {
        Test-Preflight
        Invoke-Docker ($composeUp + @('up', '-d') + $extra)
    }
    'down' {
        if ($Volumes) {
            Confirm-Volumes
            Invoke-Docker ($composeAll + @('down', '-v') + $extra)
            Remove-MachineKey
        } else {
            Invoke-Docker ($composeAll + @('down') + $extra)
        }
    }
    { $_ -in @('restart', 'ps') } { Invoke-Docker ($composeAll + @($command) + $extra) }
    'logs' { Invoke-Docker ($composeAll + @('logs', '-f') + $extra) }
    'config' { Invoke-Docker ($composeUp + @('config', '--no-env-resolution') + $extra) }
    'build' { Invoke-Docker ($composeUp + @('build') + $extra) }
}
exit 0
