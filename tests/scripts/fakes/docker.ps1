#!/usr/bin/env pwsh
# Fake docker for the operator script tests, called by the PowerShell halves. Mirrors the
# Bash fake `docker` next to it; see there for the environment variables it reads.

$line = $args -join ' '
if ($env:POSTIT_FAKE_DOCKER_LOG) { [IO.File]::AppendAllText($env:POSTIT_FAKE_DOCKER_LOG, "$line`n") }
if ($env:FAKE_DOCKER_FAIL -and $line -cmatch $env:FAKE_DOCKER_FAIL) { exit 1 }

switch ($args[0]) {
    'inspect' {
        if ($line -like '*config_files*') {
            if (-not $env:FAKE_DOCKER_CONFIG_FILES) { exit 1 }
            $env:FAKE_DOCKER_CONFIG_FILES
        } elseif ($line -like '*State.Running*') {
            if ($env:FAKE_DOCKER_RUNNING) { $env:FAKE_DOCKER_RUNNING } else { 'true' }
        }
    }
    'ps' {
        if ($env:FAKE_DOCKER_PS) { $env:FAKE_DOCKER_PS -split '\s+' | Where-Object { $_ } }
    }
    'cp' {
        if ("$($args[1])".StartsWith('postit-postgres:')) { [IO.File]::WriteAllBytes($args[2], [byte[]]@()) }
    }
    'exec' {
        if ($env:FAKE_DOCKER_STDOUT) { $env:FAKE_DOCKER_STDOUT }
    }
    'compose' {
        if ($line -like '* --services*') {
            $services = if ($env:FAKE_DOCKER_SERVICES) { $env:FAKE_DOCKER_SERVICES } else { 'postit-postgres postit-zitadel postit-server' }
            $services -split '\s+' | Where-Object { $_ }
        }
    }
}
exit 0
