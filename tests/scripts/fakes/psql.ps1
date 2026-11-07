#!/usr/bin/env pwsh
$a = @($args)
& (Join-Path $PSScriptRoot tool.ps1) psql @a
exit $LASTEXITCODE
