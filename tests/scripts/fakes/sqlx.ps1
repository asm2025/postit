#!/usr/bin/env pwsh
$a = @($args)
& (Join-Path $PSScriptRoot tool.ps1) sqlx @a
exit $LASTEXITCODE
