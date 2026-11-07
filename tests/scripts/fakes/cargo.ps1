#!/usr/bin/env pwsh
$a = @($args)
& (Join-Path $PSScriptRoot tool.ps1) cargo @a
exit $LASTEXITCODE
