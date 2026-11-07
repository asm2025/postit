#!/usr/bin/env pwsh
# Fake host tool (sqlx, cargo, psql, curl) for the operator script tests, called by the PowerShell
# halves. Mirrors tool.sh; see there.
# No param block: one would swallow a `--` argument.
$Tool = $args[0]
$Rest = @($args | Select-Object -Skip 1)
$db = "$env:DATABASE_URL" -replace '^.*/', '' -replace '\?.*$', ''
if ($env:POSTIT_FAKE_DOCKER_LOG) {
    $in = Split-Path -Leaf (Get-Location).ProviderPath
    [IO.File]::AppendAllText($env:POSTIT_FAKE_DOCKER_LOG, "$Tool $($Rest -join ' ') db=$db in=$in offline=$env:SQLX_OFFLINE`n")
}
if ([Environment]::GetEnvironmentVariable("FAKE_$($Tool.ToUpper())_FAIL") -eq '1') { exit 1 }
exit 0
