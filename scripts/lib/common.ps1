# Shared by the root operator scripts (compose, db, vault, doctor, cert). Dot-sourced,
# never run. The twin is common.sh; keep the two in step.
#
# A root script sets $ScriptName, then dot-sources this file.

# The repository root, absolute, with forward slashes so both twins hand docker the
# same paths.
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path -replace '\\', '/'

# The extracted vault: POSTIT_VAULT_DIR, or !ref/vault.
$VaultDir = if ($env:POSTIT_VAULT_DIR) { $env:POSTIT_VAULT_DIR } else { "$RepoRoot/!ref/vault" }

# docker (POSTIT_DOCKER overrides, for tests). Every script calls Docker through it.
$Docker = if ($env:POSTIT_DOCKER) { $env:POSTIT_DOCKER } else { 'docker' }

# Prints "<script>: <message>" to stderr and exits 1.
function Stop-Script([string]$Message) {
    [Console]::Error.WriteLine("${ScriptName}: $Message")
    exit 1
}

# Prints "<script>: <message>" to stdout.
function Write-Say([string]$Message) {
    [Console]::Out.WriteLine("${ScriptName}: $Message")
}

# The word when it names an environment, else an empty string.
function Test-Environment([string]$Word) {
    if (@('development', 'qa', 'production') -ccontains $Word) { return $Word }
    return ''
}

# KEY's value in a dotenv file, read as data (a vault file is never dot-sourced): the last
# matching line, a trailing CR dropped, one pair of surrounding single or double quotes
# removed, matching postit-config and Compose. Empty when the file or key is missing.
function Get-EnvValue([string]$File, [string]$Key) {
    if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { return '' }
    $line = Get-Content -LiteralPath $File | Where-Object { $_ -cmatch "^\s*$([regex]::Escape($Key))=" } | Select-Object -Last 1
    if ($null -eq $line) { return '' }
    $value = $line.Substring($line.IndexOf('=') + 1).TrimEnd("`r")
    if ($value.Length -ge 2 -and (($value[0] -eq "'" -and $value[-1] -eq "'") -or ($value[0] -eq '"' -and $value[-1] -eq '"'))) {
        $value = $value.Substring(1, $value.Length - 2)
    }
    return $value
}

# The vault files an environment needs, one per project.
function Get-VaultFiles([string]$Environment) {
    if ($Environment -eq 'development') { return @('postgres.env', 'postit.env', 'zitadel.env') }
    return @('postgres.env', 'postit.env')
}

# The compose arguments for an environment: the base file plus exactly one environment
# file, always.
function Get-ComposeArgs([string]$Environment) {
    return @('compose', '-f', "$RepoRoot/docker/docker-compose.yml", '-f', "$RepoRoot/docker/docker-compose.$Environment.yml")
}

# Runs docker and exits with its code when it fails.
function Invoke-Docker([string[]]$DockerArgs) {
    & $Docker @DockerArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# Prints the prompt to stderr, reads one line from standard input, and returns $true only
# when it is exactly $Expected. End of input is a "no".
function Confirm-Typed([string]$Expected, [string]$Prompt) {
    [Console]::Error.Write($Prompt)
    $typed = [Console]::In.ReadLine()
    if ($null -eq $typed) {
        [Console]::Error.WriteLine()
        return $false
    }
    return $typed.TrimEnd("`r") -ceq $Expected
}
