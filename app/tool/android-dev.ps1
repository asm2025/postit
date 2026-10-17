#Requires -Version 7
<#
.SYNOPSIS
    Prepares an Android emulator for postit development.
.DESCRIPTION
    prepare makes postit.local resolve to the host (10.0.2.2) and installs the dev CA in the
    emulator's user CA store, so the app and Chrome (which runs the Zitadel login) trust
    https://postit.local. It needs a Google APIs image: Google Play images cannot be rooted.
    The /system change is visible only when the AVD boots with -writable-system, which start does.
.EXAMPLE
    ./android-dev.ps1 list
    ./android-dev.ps1 prepare Pixel_10a_API
    ./android-dev.ps1 start Pixel_10a_API
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('list', 'prepare', 'start', 'clear', 'reset', 'help')]
    [string] $Command = 'help',
    [Parameter(Position = 1)]
    [string] $Avd = $env:POSTIT_AVD,
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$CaPath = Join-Path $RepoRoot 'docker/shared/nginx/certs/postit-dev-ca.crt'
$HostsLine = '10.0.2.2 postit.local'
$Exe = if ($IsWindows) { '.exe' } else { '' }

# Lines that mean this boot will never finish; add the message when it is seen again.
$script:BadBootPatterns = @()

function Stop-WithError([string] $Message) {
    Write-Host "error: $Message" -ForegroundColor Red
    exit 1
}

function Get-Tool([string] $RelativePath) {
    if (-not $env:ANDROID_HOME) {
        Stop-WithError 'ANDROID_HOME is not set. Point it at the Android SDK (for example D:\Android\sdk).'
    }
    $path = Join-Path $env:ANDROID_HOME "$RelativePath$Exe"
    if (-not (Test-Path $path)) { Stop-WithError "$path is missing. Install it with the Android SDK Manager." }
    $path
}

# Folders that may hold AVDs, in lookup order. The emulator searches only ANDROID_AVD_HOME, or
# ~/.android/avd when that is unset, so the script reads these and never sets the variable.
function Get-AvdDirs {
    $dirs = @()
    if ($env:ANDROID_AVD_HOME) { $dirs += [pscustomobject]@{ Path = $env:ANDROID_AVD_HOME; Searched = $true } }
    if ($env:ANDROID_USER_HOME) {
        $dirs += [pscustomobject]@{ Path = (Join-Path $env:ANDROID_USER_HOME 'avd'); Searched = $false }
    }
    $dirs += [pscustomobject]@{ Path = (Join-Path $HOME '.android/avd'); Searched = -not $env:ANDROID_AVD_HOME }
    # A folder named twice keeps its first entry, so ANDROID_AVD_HOME wins.
    $seen = @()
    foreach ($dir in $dirs) {
        $full = [IO.Path]::GetFullPath($dir.Path).TrimEnd('\', '/')
        if ($seen -notcontains $full) { $seen += $full; $dir }
    }
}

function Find-AvdDir([string] $Name) {
    foreach ($dir in Get-AvdDirs) {
        if (-not (Test-Path (Join-Path $dir.Path "$Name.avd/config.ini"))) { continue }
        if (-not $dir.Searched) {
            Stop-WithError "The emulator does not search $($dir.Path). Set ANDROID_AVD_HOME=$($dir.Path) (user environment variable) and open a new terminal."
        }
        return $dir.Path
    }
    Stop-WithError "No AVD named '$Name' in $((Get-AvdDirs).Path -join ', '). Run: ./android-dev.ps1 list"
}

function Get-AvdTag([string] $Name, [string] $Dir) {
    if (-not $Dir) { $Dir = Find-AvdDir $Name }
    $ini = Join-Path $Dir "$Name.avd/config.ini"
    $line = Get-Content $ini | Where-Object { $_ -like 'tag.id=*' } | Select-Object -First 1
    if ($line) { $line.Substring('tag.id='.Length) } else { 'unknown' }
}

function Assert-AvdName {
    if (-not $Avd) { Stop-WithError 'Name the AVD (positional argument) or set POSTIT_AVD. Run: ./android-dev.ps1 list' }
}

function Invoke-Adb([string] $Serial, [string[]] $Arguments) {
    $output = & $script:Adb -s $Serial @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { Stop-WithError "adb $($Arguments -join ' ') failed: $output" }
    $output
}

function Find-Serial([string] $Name) {
    $lines = & $script:Adb devices | Select-Object -Skip 1
    foreach ($line in $lines) {
        if ($line -match '^(emulator-\d+)\s+device') {
            $serial = $Matches[1]
            $running = (& $script:Adb -s $serial emu avd name 2>$null | Select-Object -First 1)
            if ($running -and $running.Trim() -eq $Name) { return $serial }
        }
    }
    $null
}

function Get-BootMinutes {
    if (-not $env:POSTIT_BOOT_MINUTES) { return 20 }
    $minutes = 0
    if (-not [int]::TryParse($env:POSTIT_BOOT_MINUTES, [ref] $minutes) -or $minutes -lt 1) {
        Stop-WithError "POSTIT_BOOT_MINUTES must be a whole number of minutes, not '$env:POSTIT_BOOT_MINUTES'."
    }
    $minutes
}

function Get-LogPath { Join-Path ([IO.Path]::GetTempPath()) "postit-emulator-$Avd.log" }

function Show-LogTail {
    $log = Get-LogPath
    if (Test-Path $log) { Get-Content $log -Tail 40 | Write-Host }
}

function Assert-GoodBoot([string] $Serial) {
    if (-not $script:BadBootPatterns) { return }
    $log = Get-LogPath
    if (-not (Test-Path $log)) { return }
    $match = Select-String -Path $log -Pattern $script:BadBootPatterns -SimpleMatch | Select-Object -First 1
    if (-not $match) { return }
    if ($Serial) { & $script:Adb -s $Serial emu kill 2>&1 | Out-Null }
    Write-Host $match.Line
    Stop-WithError 'Bad boot detected; run start again.'
}

function Wait-Boot([string] $Serial) {
    $minutes = Get-BootMinutes
    $deadline = (Get-Date).AddMinutes($minutes)
    & $script:Adb -s $Serial wait-for-device | Out-Null
    while ((Get-Date) -lt $deadline) {
        $done = (& $script:Adb -s $Serial shell getprop sys.boot_completed 2>$null) -join ''
        if ($done.Trim() -eq '1') { return }
        Assert-GoodBoot $Serial
        Start-Sleep -Seconds 1
    }
    Show-LogTail
    Stop-WithError "$Avd did not finish booting in $minutes minutes. Full log: $(Get-LogPath)"
}

function Start-Avd([switch] $Wipe) {
    $existing = Find-Serial $Avd
    if ($existing) {
        if (-not $Wipe) { Write-Host "$Avd is already running as $existing."; return $existing }
        & $script:Adb -s $existing emu kill | Out-Null
        while (Find-Serial $Avd) { Start-Sleep -Seconds 1 }
    }
    $arguments = "-avd $Avd -writable-system"
    if ($Wipe) { $arguments += ' -wipe-data' }
    # Start-Process cannot send stdout and stderr to one file, so a shell does the redirect.
    $log = Get-LogPath
    $started = Get-Date
    if ($IsWindows) {
        $line = "`"`"$script:Emulator`" $arguments > `"$log`" 2>&1`""
        Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $line -WindowStyle Hidden | Out-Null
    } else {
        Start-Process -FilePath 'sh' -ArgumentList '-c', "'exec `"$script:Emulator`" $arguments > `"$log`" 2>&1'" | Out-Null
    }
    Write-Host "Starting $Avd (log: $log) ..."
    for ($i = 0; $i -lt 120; $i++) {
        $serial = Find-Serial $Avd
        if ($serial) {
            Wait-Boot $serial
            Write-Host ('Booted {0} in {1:0} s.' -f $Avd, ((Get-Date) - $started).TotalSeconds)
            return $serial
        }
        Assert-GoodBoot $null
        Start-Sleep -Seconds 1
    }
    Show-LogTail
    Stop-WithError "$Avd did not show up in adb devices within 2 minutes. Full log: $log"
}

# adb root restarts adbd, so the connection may report closed even though it worked.
function Enable-Root([string] $Serial) {
    & $script:Adb -s $Serial root 2>&1 | Out-Null
    Start-Sleep -Seconds 2
    Wait-Boot $Serial
    $uid = (& $script:Adb -s $Serial shell id -u 2>$null) -join ''
    if ($uid.Trim() -ne '0') { Stop-WithError "adb root did not give root on $Serial. Use a Google APIs image." }
}

function Restart-AsRoot([string] $Serial) {
    Invoke-Adb $Serial @('reboot') | Out-Null
    Wait-Boot $Serial
    Enable-Root $Serial
}

# subject_hash_old: the MD5 of the DER-encoded subject, first four bytes read little-endian.
function Get-SubjectHashOld([string] $Path) {
    $loader = 'System.Security.Cryptography.X509Certificates.X509CertificateLoader' -as [type]
    $cert = if ($loader) { $loader::LoadCertificateFromFile($Path) }
            else { [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($Path) }
    $md5 = [System.Security.Cryptography.MD5]::HashData($cert.SubjectName.RawData)
    '{0:x8}' -f [BitConverter]::ToUInt32($md5, 0)
}

function Invoke-Prepare {
    $tag = Get-AvdTag $Avd
    if ($tag -like '*playstore*') {
        Stop-WithError "$Avd uses a Google Play image ($tag), which cannot be rooted. Create an AVD with a Google APIs image."
    }
    if (-not (Test-Path $CaPath)) { Stop-WithError "$CaPath is missing. Run ./cert.ps1 from the repo root first." }

    $serial = Start-Avd
    Enable-Root $serial
    Invoke-Adb $serial @('shell', 'avbctl', 'disable-verification') | Out-Null
    Restart-AsRoot $serial
    $remount = (Invoke-Adb $serial @('remount')) -join "`n"
    if ($remount -match 'reboot') {
        Restart-AsRoot $serial
        Invoke-Adb $serial @('remount') | Out-Null
    }

    $hosts = (Invoke-Adb $serial @('shell', 'cat', '/system/etc/hosts')) -join "`n"
    if ($hosts -notmatch [regex]::Escape($HostsLine)) {
        Invoke-Adb $serial @('shell', "echo '$HostsLine' >> /system/etc/hosts") | Out-Null
        Write-Host "Added '$HostsLine' to /system/etc/hosts."
    } else {
        Write-Host 'postit.local is already in /system/etc/hosts.'
    }

    $hash = Get-SubjectHashOld $CaPath
    $target = "/data/misc/user/0/cacerts-added/$hash.0"
    Invoke-Adb $serial @('push', $CaPath, '/data/local/tmp/postit-dev-ca.crt') | Out-Null
    Invoke-Adb $serial @('shell', "mkdir -p /data/misc/user/0/cacerts-added && cp /data/local/tmp/postit-dev-ca.crt $target && chown system:system $target && chmod 644 $target && restorecon $target && rm /data/local/tmp/postit-dev-ca.crt") | Out-Null
    Write-Host "Installed the dev CA as $target."
    Write-Host "$Avd is ready. Keep starting it with: ./android-dev.ps1 start $Avd"
}

function Invoke-Clear {
    if (-not $Force) {
        $answer = Read-Host "Wipe all data on $Avd? Type yes to continue"
        if ($answer -ne 'yes') { Write-Host 'Cancelled.'; exit 0 }
    }
    Start-Avd -Wipe | Out-Null
    Write-Host "$Avd was wiped and is running."
}

function Show-List {
    $found = $false
    foreach ($dir in Get-AvdDirs) {
        $items = Get-ChildItem -Path $dir.Path -Filter '*.ini' -File -ErrorAction SilentlyContinue
        foreach ($item in $items) {
            if (-not (Test-Path (Join-Path $dir.Path "$($item.BaseName).avd/config.ini"))) { continue }
            $found = $true
            $tag = Get-AvdTag $item.BaseName $dir.Path
            $note = if ($tag -like '*playstore*') { 'cannot be prepared (Google Play image)' } else { 'can be prepared' }
            '{0,-28} {1,-32} {2,-40} {3}' -f $item.BaseName, $tag, $note, $dir.Path
        }
    }
    if (-not $found) { Write-Host "No AVDs in $((Get-AvdDirs).Path -join ', ')." }
}

function Show-Help {
    @'
Usage: ./android-dev.ps1 <command> [avd] [-Force]

  list            List AVDs, whether they can be prepared, and the folder each is in.
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry) and print
                  how long the boot took.
  clear <avd>     Wipe the AVD's data. Asks first unless -Force is given.
  reset <avd>     clear, then prepare.
  help            Show this text.

The AVD defaults to $env:POSTIT_AVD. Tools come from ANDROID_HOME.

AVDs are looked up in ANDROID_AVD_HOME, then ANDROID_USER_HOME/avd, then ~/.android/avd.
The script never sets ANDROID_AVD_HOME. The emulator searches only ANDROID_AVD_HOME (or
~/.android/avd when it is unset), so an AVD found anywhere else stops the script with a
request to set ANDROID_AVD_HOME.

The emulator console goes to <temp>/postit-emulator-<avd>.log, rewritten on each start.
A boot that takes longer than POSTIT_BOOT_MINUTES (default 20) stops with the end of that log.
'@
}

if ($Command -eq 'help') { Show-Help; exit 0 }
if ($Command -eq 'list') { Show-List; exit 0 }
Assert-AvdName
Find-AvdDir $Avd | Out-Null
$script:Adb = Get-Tool 'platform-tools/adb'
$script:Emulator = Get-Tool 'emulator/emulator'
switch ($Command) {
    'prepare' { Invoke-Prepare }
    'start' { $serial = Start-Avd; Write-Host "$Avd is running as $serial." }
    'clear' { Invoke-Clear }
    'reset' { Invoke-Clear; Invoke-Prepare }
}
