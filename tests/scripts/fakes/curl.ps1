#!/usr/bin/env pwsh
$a = @($args)
& (Join-Path $PSScriptRoot tool.ps1) curl @a
exit $LASTEXITCODE
