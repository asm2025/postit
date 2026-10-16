#!/usr/bin/env bash
# Prepares an Android emulator for postit development. See android-dev.ps1 for details;
# both scripts offer the same commands.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
ca_path="$repo_root/docker/shared/nginx/certs/postit-dev-ca.crt"
hosts_line='10.0.2.2 postit.local'
command="${1:-help}"
avd="${2:-${POSTIT_AVD:-}}"
force=false
for arg in "$@"; do [[ "$arg" == "--force" ]] && force=true; done

die() { echo "error: $*" >&2; exit 1; }

avd_home() {
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then echo "$ANDROID_AVD_HOME"
  elif [[ -n "${ANDROID_USER_HOME:-}" ]]; then echo "$ANDROID_USER_HOME/avd"
  else echo "$HOME/.android/avd"; fi
}

tool() {
  [[ -n "${ANDROID_HOME:-}" ]] || die "ANDROID_HOME is not set. Point it at the Android SDK."
  local path="$ANDROID_HOME/$1"
  [[ -x "$path" ]] || die "$path is missing. Install it with the Android SDK Manager."
  echo "$path"
}

avd_tag() {
  local ini; ini="$(avd_home)/$1.avd/config.ini"
  [[ -f "$ini" ]] || die "No AVD named '$1' in $(avd_home). Run: ./android-dev.sh list"
  grep -m1 '^tag.id=' "$ini" | cut -d= -f2 || echo unknown
}

require_avd() { [[ -n "$avd" ]] || die "Name the AVD or set POSTIT_AVD. Run: ./android-dev.sh list"; }

find_serial() {
  local serial
  for serial in $("$adb" devices | awk 'NR>1 && $2=="device" && $1 ~ /^emulator-/ {print $1}'); do
    if [[ "$("$adb" -s "$serial" emu avd name 2>/dev/null | head -n1 | tr -d '\r')" == "$1" ]]; then
      echo "$serial"; return 0
    fi
  done
  return 0
}

wait_boot() {
  "$adb" -s "$1" wait-for-device
  for _ in $(seq 1 600); do
    [[ "$("$adb" -s "$1" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]] && return 0
    sleep 1
  done
  die "$1 did not finish booting in 10 minutes."
}

start_avd() { # $1 = wipe flag (true|false); prints the serial
  local serial; serial="$(find_serial "$avd")"
  if [[ -n "$serial" ]]; then
    if [[ "$1" != true ]]; then echo "$serial"; return 0; fi
    "$adb" -s "$serial" emu kill >/dev/null
    while [[ -n "$(find_serial "$avd")" ]]; do sleep 1; done
  fi
  local args=(-avd "$avd" -writable-system)
  [[ "$1" == true ]] && args+=(-wipe-data)
  nohup "$emulator" "${args[@]}" >/dev/null 2>&1 &
  for _ in $(seq 1 120); do
    serial="$(find_serial "$avd")"
    if [[ -n "$serial" ]]; then wait_boot "$serial"; echo "$serial"; return 0; fi
    sleep 1
  done
  die "$avd did not show up in adb devices within 2 minutes."
}

# adb root restarts adbd, so the connection may report closed even though it worked.
enable_root() {
  "$adb" -s "$1" root >/dev/null 2>&1 || true
  sleep 2
  wait_boot "$1"
  [[ "$("$adb" -s "$1" shell id -u | tr -d '\r')" == "0" ]] || die "adb root did not give root on $1. Use a Google APIs image."
}

restart_as_root() {
  "$adb" -s "$1" reboot; wait_boot "$1"
  enable_root "$1"
}

cmd_prepare() {
  require_avd
  local tag; tag="$(avd_tag "$avd")"
  [[ "$tag" == *playstore* ]] && die "$avd uses a Google Play image ($tag), which cannot be rooted. Create an AVD with a Google APIs image."
  [[ -f "$ca_path" ]] || die "$ca_path is missing. Run ./cert.sh from the repo root first."
  command -v openssl >/dev/null || die "openssl is required to compute the CA file name."

  local serial; serial="$(start_avd false)"
  enable_root "$serial"
  "$adb" -s "$serial" shell avbctl disable-verification >/dev/null
  restart_as_root "$serial"
  if "$adb" -s "$serial" remount 2>&1 | grep -qi reboot; then
    restart_as_root "$serial"
    "$adb" -s "$serial" remount >/dev/null
  fi

  if "$adb" -s "$serial" shell cat /system/etc/hosts | grep -qF "$hosts_line"; then
    echo "postit.local is already in /system/etc/hosts."
  else
    "$adb" -s "$serial" shell "echo '$hosts_line' >> /system/etc/hosts"
    echo "Added '$hosts_line' to /system/etc/hosts."
  fi

  local hash target
  hash="$(openssl x509 -subject_hash_old -noout -in "$ca_path")"
  target="/data/misc/user/0/cacerts-added/$hash.0"
  "$adb" -s "$serial" push "$ca_path" /data/local/tmp/postit-dev-ca.crt >/dev/null
  "$adb" -s "$serial" shell "mkdir -p /data/misc/user/0/cacerts-added && cp /data/local/tmp/postit-dev-ca.crt $target && chown system:system $target && chmod 644 $target && restorecon $target && rm /data/local/tmp/postit-dev-ca.crt"
  echo "Installed the dev CA as $target."
  echo "$avd is ready. Keep starting it with: ./android-dev.sh start $avd"
}

cmd_clear() {
  require_avd
  if [[ "$force" != true ]]; then
    read -r -p "Wipe all data on $avd? Type yes to continue: " answer
    [[ "$answer" == "yes" ]] || { echo "Cancelled."; exit 0; }
  fi
  start_avd true >/dev/null
  echo "$avd was wiped and is running."
}

cmd_list() {
  local ini name tag note found=false
  for ini in "$(avd_home)"/*.ini; do
    [[ -f "$ini" ]] || continue
    found=true
    name="$(basename "$ini" .ini)"; tag="$(avd_tag "$name")"
    if [[ "$tag" == *playstore* ]]; then note="cannot be prepared (Google Play image)"; else note="can be prepared"; fi
    printf '%-28s %-32s %s\n' "$name" "$tag" "$note"
  done
  [[ "$found" == true ]] || echo "No AVDs in $(avd_home)."
}

usage() {
  cat <<'EOF'
Usage: ./android-dev.sh <command> [avd] [--force]

  list            List AVDs and whether they can be prepared.
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry).
  clear <avd>     Wipe the AVD's data. Asks first unless --force is given.
  reset <avd>     clear, then prepare.
  help            Show this text.

The AVD defaults to $POSTIT_AVD. AVDs are read from ANDROID_AVD_HOME, else
ANDROID_USER_HOME/avd, else ~/.android/avd. Tools come from ANDROID_HOME.
EOF
}

case "$command" in
  help) usage; exit 0 ;;
  list) cmd_list; exit 0 ;;
esac
# The emulator looks only in ANDROID_AVD_HOME and ~/.android/avd, not in ANDROID_USER_HOME/avd.
export ANDROID_AVD_HOME
ANDROID_AVD_HOME="$(avd_home)"
adb="$(tool platform-tools/adb)"
emulator="$(tool emulator/emulator)"
case "$command" in
  prepare) cmd_prepare ;;
  start) require_avd; echo "$avd is running as $(start_avd false)." ;;
  clear) cmd_clear ;;
  reset) cmd_clear; cmd_prepare ;;
  *) usage; exit 1 ;;
esac
