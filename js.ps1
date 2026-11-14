#!/usr/bin/env pwsh
# The JavaScript tooling of the web client (web/): install, update, build, and the quality
# gate, always through pnpm. Twin: js.sh.

#Requires -Version 7

param(
    # <command> [-c] [-l|--latest]
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'js'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
js - manage the JavaScript packages of the web client (web/) with pnpm.

USAGE
  ./js.ps1 <command> [options]

COMMANDS
  install     Install the packages (pnpm install --prefer-frozen-lockfile)
  update      Update the packages within the ranges in package.json
  self-update Update pnpm itself to the latest version (pnpm self-update)
  build       Build the web client (pnpm run build)
  check       The web quality gate: typecheck, lint, format:check, test, build;
              stops at the first failure
  clean       Remove web/node_modules and web/dist (the lockfile stays)
  list        List the projects with a package.json
  audit       Audit the packages for known vulnerabilities (pnpm audit)
  outdated    List packages with a newer version (pnpm outdated)
  help        Show this help (also -h, --help)

OPTIONS
  -c          CI mode for install: --frozen-lockfile, so a stale lockfile fails.
              On by default when CI, GITHUB_ACTIONS, GITLAB_CI, JENKINS_HOME,
              TF_BUILD, or CIRCLECI is set
  -l          For update: also cross major versions (rewrites package.json
              ranges). Same as --latest

  POSTIT_PNPM=<path>   use this pnpm executable (tests use it)

  pnpm only, never npm. On Windows Git Bash the pnpm shim can fail: run
  ./js.ps1 from PowerShell instead.

EXAMPLES
  ./js.ps1 install                      Restore the packages
  ./js.ps1 install -c                   Same, failing on a stale lockfile
  ./js.ps1 update -l                    Move every package to its latest major
  ./js.ps1 self-update                  Update pnpm itself
  ./js.ps1 check                        The gate CI runs for the web client

EXIT CODES
  0  success
  1  usage error, pnpm missing, or a step failed (pnpm's own code when it fails)

MORE
  docs/scripts.md                       every operator script and the first-run order
  web/CLAUDE.md                         the web client's commands
'@
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { 'help' }
if ($command -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($command -cnotin @('install', 'update', 'self-update', 'build', 'check', 'clean', 'list', 'audit', 'outdated')) {
    Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
    Stop-Script "unknown command: $command"
}

$ciMode = $false
$latest = $false
foreach ($arg in @($Arguments | Select-Object -Skip 1)) {
    switch -CaseSensitive ($arg) {
        '-c' { $ciMode = $true }
        { $_ -in @('-l', '--latest') } { $latest = $true }
        default {
            Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
            Stop-Script "unknown option: $arg"
        }
    }
}
if (-not $ciMode) {
    foreach ($name in @('CI', 'GITHUB_ACTIONS', 'GITLAB_CI', 'JENKINS_HOME', 'TF_BUILD', 'CIRCLECI')) {
        if ([Environment]::GetEnvironmentVariable($name)) { $ciMode = $true }
    }
}

$projects = @('web')
$pnpm = if ($env:POSTIT_PNPM) { $env:POSTIT_PNPM } else { 'pnpm' }

# Runs pnpm in the project folder, exiting with its code when it fails.
function Invoke-Pnpm([string[]]$PnpmArgs) {
    Push-Location -LiteralPath "$RepoRoot/$project"
    try {
        & $pnpm @PnpmArgs
        $code = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($code -ne 0) { exit $code }
}

if ($command -ne 'list' -and -not (Get-Command $pnpm -ErrorAction SilentlyContinue)) {
    Stop-Script 'pnpm not found; install it (corepack enable, or https://pnpm.io/installation) and re-run'
}

if ($command -eq 'self-update') {
    Write-Say 'updating pnpm'
    Push-Location -LiteralPath $RepoRoot
    try { & $pnpm self-update; $code = $LASTEXITCODE } finally { Pop-Location }
    exit $code
}

foreach ($project in $projects) {
    if (-not (Test-Path -LiteralPath "$RepoRoot/$project/package.json")) { continue }
    switch -CaseSensitive ($command) {
        'list' {
            [Console]::Out.WriteLine($project)
            [Console]::Out.WriteLine("  Path: $RepoRoot/$project")
        }
        'install' {
            if ($ciMode) {
                Write-Say "${project}: installing (CI mode, frozen lockfile)"
                Invoke-Pnpm @('install', '--frozen-lockfile')
            } else {
                Write-Say "${project}: installing"
                Invoke-Pnpm @('install', '--prefer-frozen-lockfile')
            }
        }
        'update' {
            if ($latest) {
                Write-Say "${project}: updating to the latest versions (majors may change)"
                Invoke-Pnpm @('update', '--latest')
            } else {
                Write-Say "${project}: updating within the ranges in package.json (-l crosses majors)"
                Invoke-Pnpm @('update')
            }
        }
        'build' {
            Write-Say "${project}: building"
            Invoke-Pnpm @('run', 'build')
        }
        'check' {
            foreach ($step in @('typecheck', 'lint', 'format:check', 'test', 'build')) {
                Write-Say "${project}: $step"
                Invoke-Pnpm @('run', $step)
            }
        }
        'clean' {
            foreach ($path in @('node_modules', 'dist')) {
                if (Test-Path -LiteralPath "$RepoRoot/$project/$path") {
                    Remove-Item -LiteralPath "$RepoRoot/$project/$path" -Recurse -Force
                    Write-Say "${project}: removed $path"
                }
            }
        }
        'audit' { Invoke-Pnpm @('audit') }
        'outdated' {
            # pnpm outdated exits 1 when anything is outdated; that is a finding, not a failure.
            Push-Location -LiteralPath "$RepoRoot/$project"
            try { & $pnpm outdated } finally { Pop-Location }
        }
    }
}
exit 0
