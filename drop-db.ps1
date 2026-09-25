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
# Connection: the postit-postgres container on this host, as its own POSTGRES_USER (from
# the vault) over the local socket, which the postgres image trusts — so no password is
# handled here and no host psql is needed. Set $env:ADMIN_DATABASE_URL
# (postgres://user:password@host:port/db) to reset a database elsewhere with a host psql.

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
    throw "$Environment requires -NoStop: stop postit-api and postit-worker yourself first " + `
        "(docker stop postit-api postit-worker), then re-run with -NoStop."
}

# --- Resolve the connection ---

if ($env:ADMIN_DATABASE_URL) {
    if ($env:ADMIN_DATABASE_URL -notmatch '^postgres(?:ql)?://(?<user>[^:@/]+):(?<password>[^@]*)@(?<host>[^:/]+):(?<port>\d+)/(?<db>[^?]+)') {
        throw "Could not parse ADMIN_DATABASE_URL (expected postgres://user:password@host:port/db)."
    }
    $dbName = $Matches.db
    $target = "$($Matches.host):$($Matches.port)"
    $env:PGPASSWORD = $Matches.password
    $maintenanceUrl = "postgres://$($Matches.user)@$target/postgres"
} else {
    # The database name is config (database.url in server/config/<environment>.toml),
    # the same in every environment.
    $dbName = "postit"
    $target = "the postit-postgres container"
    $running = & docker inspect --format "{{.State.Running}}" postit-postgres 2>$null
    if ($running -ne "true") {
        throw "postit-postgres is not running on this host. Start it (./stack.ps1 up $Environment), " + `
            "or set `$env:ADMIN_DATABASE_URL to reach a database elsewhere."
    }
}

# Runs one statement against the maintenance database. The statement goes to the
# container as a positional argument, so no shell ever re-parses its quotes.
function Invoke-Sql([string]$Sql) {
    if ($env:ADMIN_DATABASE_URL) {
        & psql $maintenanceUrl -v ON_ERROR_STOP=1 -c $Sql
    } else {
        & docker exec postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d postgres -v ON_ERROR_STOP=1 -c "$1"' sh $Sql
    }
    if ($LASTEXITCODE -ne 0) { throw "psql failed: $Sql" }
}

# --- Stop the migrating processes first ---

$dockerDir = Join-Path $PSScriptRoot "docker"
$composeArgs = @(
    "-f", (Join-Path $dockerDir "docker-compose.yml"),
    "-f", (Join-Path $dockerDir "docker-compose.development.yml")
)
$stoppedServices = @()

# The app-profile services that connect to Postgres (postit-server from plan 02 P6/P7
# on). Until they exist, this is a no-op — postit-nginx-app is also in the app profile but
# never connects to Postgres directly, so it is not a migrating process and stays running.
$migratingServiceCandidates = @("postit-server", "postit-api", "postit-worker")

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

Write-Host "Dropping database '$dbName' on $target..."
Invoke-Sql "DROP DATABASE IF EXISTS `"$dbName`" WITH (FORCE);"
# No OWNER clause: the connecting role owns it, as it owned the one just dropped.
Invoke-Sql "CREATE DATABASE `"$dbName`";"
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
