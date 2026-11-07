#!/usr/bin/env pwsh
# The encrypted vault archive, !ref/vault.7z. Twin: vault.sh.

#Requires -Version 7

param(
    [switch]$Force,
    # <command>
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$ScriptName = 'vault'
. (Join-Path $PSScriptRoot 'scripts/lib/common.ps1')

function Show-Usage {
    @'
vault - pack, unpack, and check the encrypted vault archive, !ref/vault.7z.

USAGE
  ./vault.ps1 <command> [-Force]

COMMANDS
  unpack    Extract !ref/vault.7z into !ref/vault/. Refuses when !ref/vault/
            already holds files, unless -Force
  pack      Re-pack !ref/vault/ into !ref/vault.7z (AES-256, file names encrypted
            too), every environment included, .gitkeep left out. The old archive
            is replaced only when 7-Zip succeeds
  check     Compare the archive with !ref/vault/ file by file (SHA-256) and list
            the files that differ, by name only
  help      Show this help (also -h, --help, or no command)

  7-Zip asks for the password itself; it never becomes an argument.

OPTIONS
  -Force    unpack: overwrite the files already in !ref/vault/

EXAMPLES
  ./vault.ps1 unpack                    First run on a new machine
  ./vault.ps1 check                     Did anything change since the last pack?
  ./vault.ps1 pack                      After editing a vault file; then commit
                                        !ref/vault.7z

EXIT CODES
  0  success; check: the archive matches !ref/vault/
  1  usage error, a refused guard, or a failure (a wrong password included)
  2  check: the archive and !ref/vault/ differ

MORE
  docs/scripts.md                       every operator script and the recipes
  README.md, "Environment files and secrets"
  Every committed archive stays readable with the password it was made with, so
  rotating a leaked secret means rotating the secret itself. pack keeps
  production/ until plan 03 G1 picks a secret store.
'@
}

$command = if ($Arguments.Count -gt 0) { $Arguments[0] } else { 'help' }
if ($command -in @('help', '-h', '--help')) {
    Show-Usage
    exit 0
}
if ($command -cnotin @('unpack', 'pack', 'check')) {
    Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }
    Stop-Script "unknown command: $command"
}
foreach ($arg in @($Arguments | Select-Object -Skip 1)) { Stop-Script "unknown argument: $arg" }

$vault = $VaultDir -replace '\\', '/'
$parent = Split-Path -Parent $vault
$name = Split-Path -Leaf $vault
$archive = if ($env:POSTIT_VAULT_ARCHIVE) { $env:POSTIT_VAULT_ARCHIVE } else { "$RepoRoot/!ref/vault.7z" }

# POSTIT_7Z, else 7z or 7za on PATH, else the Windows installer's default location.
function Find-SevenZip {
    if ($env:POSTIT_7Z) {
        if (Get-Command $env:POSTIT_7Z -ErrorAction SilentlyContinue) { return $env:POSTIT_7Z }
        return ''
    }
    foreach ($candidate in @('7z', '7za')) {
        $found = Get-Command $candidate -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { return $found.Source }
    }
    if (Test-Path -LiteralPath 'C:/Program Files/7-Zip/7z.exe') { return 'C:/Program Files/7-Zip/7z.exe' }
    return ''
}

$sevenZip = Find-SevenZip
if (-not $sevenZip) {
    Stop-Script '7-Zip is not installed: get it from https://www.7-zip.org/ (Linux: p7zip-full), or open !ref/vault.7z in the 7-Zip GUI as README.md describes'
}

# The password is 7-Zip's own prompt. POSTIT_7Z_TEST_PASSWORD exists for the tests only.
$passwordArg = if ($env:POSTIT_7Z_TEST_PASSWORD) { "-p$env:POSTIT_7Z_TEST_PASSWORD" } else { '-p' }

function Invoke-SevenZip([string[]]$SevenZipArgs) {
    & $sevenZip @SevenZipArgs | Out-Null
    return $LASTEXITCODE -eq 0
}

# The extracted tree: the archive's own vault/ folder, or its top when it has none.
function Get-TreeRoot([string]$Dir) {
    if (Test-Path -LiteralPath "$Dir/$name" -PathType Container) { return "$Dir/$name" }
    return $Dir
}

# Relative paths of the files under a directory, .gitkeep left out, sorted.
function Get-Files([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir -PathType Container)) { return @() }
    $full = (Resolve-Path -LiteralPath $Dir).ProviderPath
    return @(Get-ChildItem -LiteralPath $full -Recurse -File -Force |
            Where-Object { $_.Name -ne '.gitkeep' } |
            ForEach-Object { $_.FullName.Substring($full.Length).TrimStart('\', '/') -replace '\\', '/' } |
            Sort-Object -CaseSensitive)
}

function Invoke-Unpack {
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { Stop-Script "no archive at $archive" }
    New-Item -ItemType Directory -Force $vault | Out-Null
    if (-not $Force -and (Get-Files $vault).Count -gt 0) {
        Stop-Script "$vault already holds files; run ./vault.ps1 check to compare, or unpack -Force to overwrite them"
    }
    # Extracted next to the files (inside the git-ignored vault folder), then moved in.
    $staging = "$vault/.unpack.$PID"
    try {
        if (-not (Invoke-SevenZip @('x', $passwordArg, "-o$staging", $archive))) {
            Stop-Script "7-Zip could not extract $archive (a wrong password?); $vault is unchanged"
        }
        $source = Get-TreeRoot $staging
        Get-ChildItem -LiteralPath $source -Force | Copy-Item -Destination $vault -Recurse -Force
    } finally {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Say "unpacked into $vault"
}

function Invoke-Pack {
    if ((Get-Files $vault).Count -eq 0) { Stop-Script "nothing to pack: $vault holds no files" }
    $tmp = "$archive.tmp.$PID.7z"
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    $ok = $false
    try {
        # From the vault's parent, so every entry sits under vault/ in the archive.
        Push-Location -LiteralPath $parent
        try {
            $ok = Invoke-SevenZip @('a', $passwordArg, '-mhe=on', '-xr!.gitkeep', $tmp, $name)
        } finally {
            Pop-Location
        }
        if ($ok) { Move-Item -LiteralPath $tmp -Destination $archive -Force }
    } finally {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
    if (-not $ok) { Stop-Script "7-Zip failed; $archive is unchanged" }
    Write-Say "packed $vault into $archive; commit it"
}

function Invoke-Check {
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { Stop-Script "no archive at $archive" }
    # Outside the repository and owner-only; TMPDIR first, as mktemp does.
    $base = if ($env:TMPDIR) { $env:TMPDIR } else { [IO.Path]::GetTempPath() }
    $tmp = (Join-Path $base "postit-vault-check.$([guid]::NewGuid().ToString('N'))") -replace '\\', '/'
    New-Item -ItemType Directory $tmp | Out-Null
    $differ = $false
    try {
        if ($IsWindows) {
            & icacls $tmp /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" | Out-Null
        } else {
            & chmod 700 $tmp
        }
        if (-not (Invoke-SevenZip @('x', $passwordArg, "-o$tmp", $archive))) {
            Stop-Script "7-Zip could not extract $archive (a wrong password?)"
        }
        $a = Get-TreeRoot $tmp
        $files = @((Get-Files $a) + (Get-Files $vault) | Sort-Object -Unique -CaseSensitive)
        foreach ($file in $files) {
            if (-not (Test-Path -LiteralPath "$a/$file" -PathType Leaf)) {
                [Console]::Out.WriteLine("only in ${vault}: $file")
                $differ = $true
            } elseif (-not (Test-Path -LiteralPath "$vault/$file" -PathType Leaf)) {
                [Console]::Out.WriteLine("only in the archive: $file")
                $differ = $true
            } elseif ((Get-FileHash -LiteralPath "$a/$file" -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath "$vault/$file" -Algorithm SHA256).Hash) {
                [Console]::Out.WriteLine("changed: $file")
                $differ = $true
            }
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($differ) {
        Write-Say "the archive and $vault differ; ./vault.ps1 pack to update the archive"
        exit 2
    }
    Write-Say "the archive matches $vault"
}

switch -CaseSensitive ($command) {
    'unpack' { Invoke-Unpack }
    'pack' { Invoke-Pack }
    'check' { Invoke-Check }
}
exit 0
