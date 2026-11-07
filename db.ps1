#!/usr/bin/env pwsh
# The postit database: psql, backup, restore, drop, and the sqlx cache. Twin: db.sh.

#Requires -Version 7

param(
    [switch]$Force,
    [switch]$NoStop,
    # <command> <environment> [file]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'db'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
db - work with the postit database of one environment.

USAGE
  ./db.ps1 <command> <environment> [options]
  ./db.ps1 restore <environment> <file> [-Force]
  ./db.ps1 prepare

COMMANDS
  psql      Open psql inside postit-postgres, as the container's own user over the
            local socket (no password is handled)
  backup    Dump the database to !ref/backups/postit-<environment>-<UTC>.dump
  restore   Restore a dump in one transaction; a failure leaves the database as it
            was. Refuses while the server runs; starts nothing afterwards
  drop      Drop the postit database and recreate it empty; the server recreates
            the schema on its next boot. The zitadel database is never touched
  prepare   Refresh every crate's sqlx offline cache (backend/crates/*/.sqlx)
            against a scratch database, postit_prepare, built from the current
            migration on the development server, then drop it (also on failure).
            Takes no environment; needs sqlx-cli
  help      Show this help (also -h, --help, or no command)

  psql, backup, and restore act on the running postit-postgres and refuse when it
  belongs to another environment than the one you name.

OPTIONS
  environment   development, qa, or production (required)
  -Force        restore: required to overwrite the production database.
                drop: skip the typed confirmation in development and qa;
                production always asks and refuses -Force
  -NoStop       drop: you stopped postit-api and postit-worker yourself. Required
                in qa and production; in development the script stops and restarts
                the running app services itself

  POSTIT_BACKUP_DIR    backup destination; inside this repository it must be a
                       git-ignored path (a dump is unencrypted user data)
  ADMIN_DATABASE_URL   drop: postgres://user:password@host:port/db to reach a
                       database elsewhere through a host psql

EXAMPLES
  ./db.ps1 psql development
  ./db.ps1 backup development
  ./db.ps1 restore development '!ref/backups/postit-development-20261107T120000Z.dump'
  ./db.ps1 drop development
  ./db.ps1 drop qa -NoStop
  ./db.ps1 prepare                      after editing a query or the migration

EXIT CODES
  0  success
  1  usage error, a refused guard, an unconfirmed question, or a failure

MORE
  docs/scripts.md                       every operator script and the recipes
  docs/deployment.md                    backups and restores outside development
'@
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { 'help' }
$rest = @($Arguments | Select-Object -Skip 1)

if ($command -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($command -cnotin @('psql', 'backup', 'restore', 'drop', 'prepare')) {
    Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
    Stop-Script "unknown command: $command"
}

if ($command -eq 'prepare') {
    if ($rest.Count -gt 0) { Stop-Script 'usage: ./db.ps1 prepare  (it takes no environment: it always uses the development server)' }
    $environment = 'development'
    $positional = @()
} else {
    $environment = if ($rest.Count -gt 0) { Test-Environment $rest[0] } else { '' }
    if (-not $environment) {
        if ($command -eq 'restore') {
            Stop-Script 'usage: ./db.ps1 restore <environment> <file> [-Force]  (<environment> is development, qa, or production)'
        }
        Stop-Script "usage: ./db.ps1 $command <environment>  (<environment> is development, qa, or production)"
    }
    $positional = @($rest | Select-Object -Skip 1)
}
foreach ($arg in $positional) {
    if ($arg.StartsWith('-')) { Stop-Script "unknown option: $arg" }
}

$composeAll = (Get-ComposeArgs $environment) + @('--profile', '*')

# Runs a docker query and returns its output; a failure gives an empty result.
function Get-DockerOutput([string[]]$DockerArgs) {
    $output = & $Docker @DockerArgs 2>$null
    if ($LASTEXITCODE -ne 0) { return @() }
    return @($output)
}

# The dump is unencrypted user data: never leave it in the container's /tmp.
function Remove-ContainerDump {
    & $Docker exec postit-postgres sh -c 'rm -f /tmp/postit.dump' 2>&1 | Out-Null
}

# The environment the running postit-postgres was started for, read from the compose
# config-file label (".../docker-compose.<environment>.yml"). Empty when there is no
# container or no label.
function Get-RunningEnvironment {
    $files = Get-DockerOutput @('inspect', 'postit-postgres', '--format', '{{index .Config.Labels "com.docker.compose.project.config_files"}}')
    $found = @(($files -join ',') -split ',' | ForEach-Object { if ($_ -match 'docker-compose\.(development|qa|production)\.yml\s*$') { $Matches[1] } })
    if ($found.Count -eq 0) { return '' }
    return $found[-1]
}

# psql, backup and restore act on that container whatever environment was typed, so
# they check the two agree first.
function Assert-RunningEnvironment {
    $running = Get-RunningEnvironment
    if (-not $running) {
        Stop-Script 'cannot tell which environment postit-postgres belongs to (no such container, or it was not started by ./compose.ps1); refusing to touch it'
    }
    if ($running -ne $environment) {
        Stop-Script "you asked for $environment, but the running postit-postgres belongs to $running; refusing"
    }
}

function Invoke-Psql {
    Assert-RunningEnvironment
    # The container's own POSTGRES_USER (from the vault) over the local socket, which the
    # postgres image trusts: no password on the command line or in the shell.
    Invoke-Docker @('exec', '-it', 'postit-postgres', 'sh', '-c', 'exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"')
}

function Invoke-Backup {
    Assert-RunningEnvironment
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMdd'T'HHmmss'Z'")
    $backupDir = if ($env:POSTIT_BACKUP_DIR) { $env:POSTIT_BACKUP_DIR } else { "$RepoRoot/!ref/backups" }
    # Absolute, from the shell's location (not .NET's), so git, New-Item and docker cp agree.
    $dir = [IO.Path]::GetFullPath($backupDir, (Get-Location).ProviderPath) -replace '\\', '/'
    $file = "$dir/postit-$environment-$stamp.dump"
    # Checked before anything is created: a refused destination gets no new directory.
    # check-ignore exits 0 (ignored), 1 (inside the work tree, not ignored) or 128 (outside
    # any work tree, or no git); only the inside case needs the rail.
    & git -C $RepoRoot check-ignore -q $file 2>$null
    $ignoreCode = $LASTEXITCODE
    if ($ignoreCode -eq 1) {
        Stop-Script "refusing to write a dump to ${file}: it is inside the repository and git does not ignore that path, and a dump is unencrypted user data"
    } elseif ($ignoreCode -ne 0) {
        Write-Say "$dir is outside the repository's git work tree, so nothing guards it from being committed; the directory is owner-only"
    }
    New-Item -ItemType Directory -Force $dir | Out-Null
    if ($IsWindows) {
        & icacls $dir /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" | Out-Null
    } else {
        & chmod 700 $dir
    }
    if ($LASTEXITCODE -ne 0) {
        Stop-Script "could not restrict $dir to the current user; refusing to write an unencrypted dump there"
    }
    $problem = $null
    try {
        # Written inside the container and copied out with docker cp: PowerShell's own
        # redirection would re-encode the binary dump as text.
        & $Docker exec postit-postgres sh -c 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump'
        if ($LASTEXITCODE -ne 0) {
            $problem = 'pg_dump failed'
        } else {
            & $Docker cp postit-postgres:/tmp/postit.dump $file
            if ($LASTEXITCODE -ne 0) {
                Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
                $problem = 'docker cp failed; no dump was kept'
            }
        }
    } finally {
        Remove-ContainerDump
    }
    if ($problem) { Stop-Script $problem }
    if ($IsWindows) {
        & icacls $file /inheritance:r /grant:r "${env:USERNAME}:F" | Out-Null
    } else {
        & chmod 600 $file
    }
    if ($LASTEXITCODE -ne 0) {
        Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
        Stop-Script "could not restrict $file to the current user; the dump was deleted"
    }
    Write-Say "wrote $file"
    Write-Say "it holds every user's data unencrypted; delete it when you are done with it"
}

function Invoke-Restore {
    $file = if ($positional.Count -gt 0) { $positional[0] } else { '' }
    if (-not $file) { Stop-Script "usage: ./db.ps1 restore <environment> <file> [-Force]  (<file> is a postit-$environment-*.dump)" }
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { Stop-Script "no such file: $file" }
    $name = Split-Path $file -Leaf
    if ($name -cnotlike "postit-$environment-*.dump") {
        Stop-Script "refusing: $name is not a postit-$environment-*.dump file"
    }
    Assert-RunningEnvironment
    if ($environment -eq 'production' -and -not $Force) {
        Stop-Script 'refusing to overwrite the production database without -Force'
    }
    $running = @(Get-DockerOutput @('ps', '--format', '{{.Names}}') | Where-Object { $_ -match '^postit-(api|worker|server)$' })
    if ($running.Count -gt 0) {
        Stop-Script "stop the server first (docker stop $($running -join ' ')): it would keep writing to a half-restored database"
    }
    $problem = $null
    try {
        & $Docker cp $file postit-postgres:/tmp/postit.dump
        if ($LASTEXITCODE -ne 0) {
            $problem = 'docker cp failed; nothing was restored'
        } else {
            & $Docker exec postit-postgres sh -c 'pg_restore --single-transaction --exit-on-error --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump'
            if ($LASTEXITCODE -ne 0) {
                $problem = 'pg_restore failed; it ran as one transaction, so the database is as it was before'
            }
        }
    } finally {
        Remove-ContainerDump
    }
    if ($problem) { Stop-Script $problem }
    Write-Say "restored $file; start the server with ./compose.ps1 up $environment"
}

# The app services that connect to Postgres. postit-nginx-app never does, so it stays up.
$migratingServices = @('postit-server', 'postit-api', 'postit-worker')

# One statement against the maintenance database. In the container the statement is a
# positional argument, so no shell re-parses its quotes.
function Invoke-Sql([string]$MaintenanceUrl, [string]$Sql) {
    if ($MaintenanceUrl) {
        & psql $MaintenanceUrl -v ON_ERROR_STOP=1 -c $Sql
    } else {
        & $Docker exec postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d postgres -v ON_ERROR_STOP=1 -c "$1"' sh $Sql
    }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

function Invoke-Drop {
    if ($environment -eq 'production' -and $Force) {
        Stop-Script 'production cannot be forced'
    }
    if ($environment -ne 'development' -and -not $NoStop) {
        Stop-Script "$environment requires -NoStop: stop postit-api and postit-worker yourself first (docker stop postit-api postit-worker), then re-run with -NoStop"
    }

    $maintenanceUrl = ''
    if ($env:ADMIN_DATABASE_URL) {
        if ($env:ADMIN_DATABASE_URL -notmatch '^postgres(?:ql)?://(?<user>[^:@/]+):(?<password>[^@]*)@(?<host>[^:/]+):(?<port>\d+)/(?<db>[^?]+)') {
            Stop-Script 'could not parse ADMIN_DATABASE_URL (expected postgres://user:password@host:port/db)'
        }
        $dbName = $Matches.db
        $target = "$($Matches.host):$($Matches.port)"
        $env:PGPASSWORD = $Matches.password
        $maintenanceUrl = "postgres://$($Matches.user)@$target/postgres"
    } else {
        # The database name is config (database.url in backend/config/<environment>.toml),
        # the same in every environment.
        $dbName = 'postit'
        $target = 'the postit-postgres container'
        $state = @(Get-DockerOutput @('inspect', '--format', '{{.State.Running}}', 'postit-postgres'))
        if ($state.Count -eq 0 -or $state[0] -ne 'true') {
            Stop-Script "postit-postgres is not running on this host; start it (./compose.ps1 up $environment), or set ADMIN_DATABASE_URL to reach a database elsewhere"
        }
    }

    # Asked before anything is stopped, so a "no" leaves everything as it was.
    if ($environment -eq 'production' -or -not $Force) {
        if ($environment -eq 'development' -and -not $NoStop) {
            [Console]::Out.WriteLine("If a native 'cargo run' server is using this database, stop it first.")
        }
        if (-not (Confirm-Typed $dbName "Type the database name ($dbName) to drop it on ${target}: ")) {
            Stop-Script 'confirmation did not match; nothing was dropped'
        }
    }

    # Development: stop the running app services, so nothing migrates mid-drop, and start
    # the same ones again afterwards.
    $stopped = @()
    if ($environment -eq 'development' -and -not $NoStop) {
        $runningNow = Get-DockerOutput ($composeAll + @('ps', '--services', '--filter', 'status=running'))
        $stopped = @($migratingServices | Where-Object { $runningNow -ccontains $_ })
        if ($stopped.Count -gt 0) {
            Write-Say "stopping $($stopped -join ' ')"
            Invoke-Docker ($composeAll + @('stop') + $stopped)
        }
    }

    Write-Say "dropping database $dbName on $target"
    Invoke-Sql $maintenanceUrl "DROP DATABASE IF EXISTS `"$dbName`" WITH (FORCE);"
    # No OWNER clause: the connecting role owns it, as it owned the one just dropped.
    Invoke-Sql $maintenanceUrl "CREATE DATABASE `"$dbName`";"
    Write-Say "database $dbName dropped and recreated empty"

    if ($stopped.Count -gt 0) {
        Write-Say "starting $($stopped -join ' ')"
        Invoke-Docker ($composeAll + @('start') + $stopped)
        $allUp = $false
        foreach ($attempt in 1..30) {
            $runningNow = Get-DockerOutput ($composeAll + @('ps', '--services', '--filter', 'status=running'))
            $allUp = -not ($stopped | Where-Object { $runningNow -cnotcontains $_ })
            if ($allUp) { break }
            Start-Sleep -Seconds 2
        }
        if (-not $allUp) {
            [Console]::Error.WriteLine('db: not every app service reported running after 60 s; check ./compose.ps1 ps')
        }
    }
}

# The sqlx offline caches (.sqlx/ next to a crate's Cargo.toml) are refreshed against a
# throwaway database built from the current migration, so a development database that
# lags the migration cannot leak into them.
$scratchDb = 'postit_prepare'

function Remove-ScratchDb {
    & $Docker exec postit-postgres sh -c 'exec psql -U "$POSTGRES_USER" -d postgres -v ON_ERROR_STOP=1 -c "$1"' sh "DROP DATABASE IF EXISTS `"$scratchDb`" WITH (FORCE);" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        [Console]::Error.WriteLine("db: could not drop the scratch database $scratchDb; drop it with ./db.ps1 psql development")
    }
}

function Invoke-Prepare {
    $sqlx = if ($env:POSTIT_SQLX) { $env:POSTIT_SQLX } else { 'sqlx' }
    $cargo = if ($env:POSTIT_CARGO) { $env:POSTIT_CARGO } else { 'cargo' }
    if (-not (Get-Command $sqlx -ErrorAction SilentlyContinue)) {
        Stop-Script 'sqlx-cli is not installed: cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres --locked'
    }
    $url = Get-EnvValue "$VaultDir/development/postgres.env" 'DATABASE_URL'
    if (-not $url) { Stop-Script 'no DATABASE_URL in !ref/vault/development/postgres.env; run ./vault.ps1 unpack first' }
    # The host as seen from this machine (localhost can resolve to ::1 first) and the
    # scratch database. The URL travels in the environment, never on a command line.
    if ($url -notmatch '^(postgres(?:ql)?://[^@/]+@)[^:/?]+(:\d+)?(/[^?]*)?(\?.*)?$') {
        Stop-Script 'DATABASE_URL in !ref/vault/development/postgres.env is not a postgres://user:password@host:port/db URL'
    }
    $url = "$($Matches[1])127.0.0.1$($Matches[2])/$scratchDb$($Matches[4])"
    $crates = @(Get-ChildItem -Directory -Force "$RepoRoot/backend/crates/*/.sqlx", "$RepoRoot/backend/crates/agents/*/.sqlx" -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Parent.FullName -replace '\\', '/' } | Sort-Object)
    if ($crates.Count -eq 0) { Stop-Script 'no crate under backend/crates has a .sqlx/ cache' }
    Assert-RunningEnvironment

    Invoke-Sql '' "DROP DATABASE IF EXISTS `"$scratchDb`" WITH (FORCE);" | Out-Null
    Invoke-Sql '' "CREATE DATABASE `"$scratchDb`";" | Out-Null
    $saved = @{ DATABASE_URL = $env:DATABASE_URL; SQLX_OFFLINE = $env:SQLX_OFFLINE }
    $problem = $null
    try {
        Write-Say "created the scratch database $scratchDb"
        $env:DATABASE_URL = $url
        $migrateArgs = @('migrate', 'run', '--source', "$RepoRoot/backend/crates/data/migrations")
        & $sqlx @migrateArgs
        if ($LASTEXITCODE -ne 0) { $problem = 'sqlx migrate run failed' }
        foreach ($crate in $crates) {
            if ($problem) { break }
            $relative = $crate.Substring($RepoRoot.Length + 1)
            Write-Say "refreshing $relative/.sqlx"
            $env:SQLX_OFFLINE = 'false'
            Push-Location -LiteralPath $crate
            try {
                # Splatted, so the `--` reaches cargo.
                $prepareArgs = @('sqlx', 'prepare', '--', '--all-targets', '--all-features')
                & $cargo @prepareArgs
                if ($LASTEXITCODE -ne 0) { $problem = "cargo sqlx prepare failed in $relative" }
            } finally {
                Pop-Location
            }
        }
    } finally {
        $env:DATABASE_URL = $saved.DATABASE_URL
        $env:SQLX_OFFLINE = $saved.SQLX_OFFLINE
        Remove-ScratchDb
    }
    if ($problem) { Stop-Script $problem }
    Write-Say 'the sqlx caches are current; review them with git status backend/crates'
}

switch -CaseSensitive ($command) {
    'prepare' { Invoke-Prepare }
    'psql' { Invoke-Psql }
    'backup' { Invoke-Backup }
    'restore' { Invoke-Restore }
    'drop' { Invoke-Drop }
}
exit 0
