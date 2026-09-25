#!/usr/bin/env pwsh
# Resets an environment's postit database to empty. Postgres only — no ECS, no bastion,
# no EF-style scaffolding. The server recreates the schema on its next boot.
#
# Usage: ./drop-db.ps1 <development|qa|production> [-Force] [-NoStop]
#   -Force    skip the retype-to-confirm prompt (development, qa only; production
#             always prompts and refuses -Force)
#   -NoStop   assert the operator already stopped the migrating processes; required
#             for qa/production, optional for development (skips the app-service stop
#             and the "stop your native cargo run" reminder)
#
# Connection: $env:ADMIN_DATABASE_URL, else !ref/vault/<environment>/postit-database-url.txt,
# else (development only) the fixed dev URL from server/config/development.toml.

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("development", "qa", "production")]
    [string]$Environment,

    [switch]$Force,
    [switch]$NoStop
)

$ErrorActionPreference = "Stop"

if ($Environment -eq "production" -and $Force) {
    throw "production cannot be forced."
}

if ($Environment -ne "development" -and -not $NoStop) {
    throw "$Environment requires -NoStop: stop 'api' and 'worker' in " + `
        "docker/docker-compose.$Environment.yaml (or scale them to zero) yourself first, " + `
        "then re-run with -NoStop."
}

# --- Resolve the connection ---

$vaultFile = Join-Path $PSScriptRoot "!ref/vault/$Environment/postit-database-url.txt"
if ($env:ADMIN_DATABASE_URL) {
    $dbUrl = $env:ADMIN_DATABASE_URL
} elseif (Test-Path $vaultFile) {
    $dbUrl = (Get-Content -Raw $vaultFile).Trim()
} elseif ($Environment -eq "development") {
    $dbUrl = "postgres://postit:postit@localhost:44340/postit"
} else {
    throw "No vault file at $vaultFile and `$env:ADMIN_DATABASE_URL is not set. " + `
        "Extract the vault first (see README: 'Extract configuration files')."
}

if ($dbUrl -notmatch '^postgres(?:ql)?://(?<user>[^:@/]+):(?<password>[^@]*)@(?<host>[^:/]+):(?<port>\d+)/(?<db>[^?]+)') {
    throw "Could not parse database URL (expected postgres://user:password@host:port/db)."
}
$dbUser = $Matches.user
$dbPassword = $Matches.password
$dbHost = $Matches.host
$dbPort = $Matches.port
$dbName = $Matches.db

$env:PGPASSWORD = $dbPassword
$maintenanceUrl = "postgres://$dbUser@${dbHost}:$dbPort/postgres"

# --- Stop the migrating processes first ---

$dockerDir = Join-Path $PSScriptRoot "docker"
$composeArgs = @(
    "--project-directory", $dockerDir,
    "-f", (Join-Path $dockerDir "docker-compose.yaml"),
    "-f", (Join-Path $dockerDir "docker-compose.dev.yaml")
)
$stoppedServices = @()

# The app-profile services that connect to Postgres (server, worker from plan 02 P6/P7
# on). Until they exist, this is a no-op — nginx-app is also in the app profile but never
# connects to Postgres directly, so it is not a migrating process and stays running.
$migratingServiceCandidates = @("server", "worker")

if (-not $NoStop -and $Environment -eq "development") {
    $allServices = & docker compose @composeArgs config --services 2>$null
    $appServices = $migratingServiceCandidates | Where-Object { $allServices -contains $_ }
    if ($appServices) {
        Write-Host "Stopping app services: $($appServices -join ', ')"
        & docker compose @composeArgs stop @appServices
        if ($LASTEXITCODE -ne 0) { throw "docker compose stop failed" }
        $stoppedServices = $appServices
    }
    if (-not $Force) {
        Write-Host "If a native 'cargo run' server is running against this database, stop it now."
        Read-Host "Press Enter to continue" | Out-Null
    }
}

# --- Confirm ---

if ($Environment -eq "production" -or -not $Force) {
    $typed = Read-Host "Type the database name ('$dbName') to confirm dropping it"
    if ($typed -ne $dbName) {
        throw "Confirmation did not match. Aborting."
    }
}

# --- Drop and recreate ---

Write-Host "Dropping database '$dbName' on ${dbHost}:${dbPort}..."
& psql $maintenanceUrl -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS `"$dbName`" WITH (FORCE);"
if ($LASTEXITCODE -ne 0) { throw "psql DROP DATABASE failed" }
& psql $maintenanceUrl -v ON_ERROR_STOP=1 -c "CREATE DATABASE `"$dbName`" OWNER `"$dbUser`";"
if ($LASTEXITCODE -ne 0) { throw "psql CREATE DATABASE failed" }
Write-Host "Database '$dbName' dropped and recreated empty."

# --- Restart ---

if ($stoppedServices.Count -gt 0) {
    Write-Host "Restarting app services: $($stoppedServices -join ', ')"
    & docker compose @composeArgs start @stoppedServices
    if ($LASTEXITCODE -ne 0) { throw "docker compose start failed" }

    Write-Host "Waiting for app services to come back up..."
    $allUp = $false
    for ($i = 0; $i -lt 30; $i++) {
        $running = & docker compose @composeArgs ps --services --filter "status=running" 2>$null
        $allUp = $true
        foreach ($s in $stoppedServices) {
            if ($running -notcontains $s) { $allUp = $false }
        }
        if ($allUp) { break }
        Start-Sleep -Seconds 2
    }
    if (-not $allUp) {
        Write-Warning "Not all app services reported running after 60s; check 'docker compose ps'."
    }
}
