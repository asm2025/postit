#!/usr/bin/env pwsh
# Fake 7-Zip for the vault tests, called by the PowerShell halves. Mirrors the Bash fake
# `7z` next to it.
$shown = @($args | ForEach-Object { if ("$_" -like '-p?*') { '-p***' } else { $_ } })
if ($env:POSTIT_FAKE_DOCKER_LOG) { [IO.File]::AppendAllText($env:POSTIT_FAKE_DOCKER_LOG, "7z $($shown -join ' ')`n") }
if ($args[0] -eq 'a') {
    $archive = $args | Select-Object -Skip 1 | Where-Object { -not "$_".StartsWith('-') } | Select-Object -First 1
    [IO.File]::WriteAllText($archive, 'partial')
    exit 2
}
exit 0
