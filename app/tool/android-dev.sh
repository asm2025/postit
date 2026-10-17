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

# Lines that mean this boot will never finish; add the message when it is seen again.
bad_boot_patterns=()

die() { echo "error: $*" >&2; exit 1; }

# Folders that may hold AVDs, in lookup order, one "searched|path" per line. The emulator
# searches only ANDROID_AVD_HOME, or ~/.android/avd when that is unset, so the script reads
# these and never sets the variable. A folder named twice keeps its first entry.
avd_dirs() {
  local entries=() entry dir seen=() s real duplicate
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then entries+=("yes|$ANDROID_AVD_HOME"); fi
  if [[ -n "${ANDROID_USER_HOME:-}" ]]; then entries+=("no|$ANDROID_USER_HOME/avd"); fi
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then entries+=("no|$HOME/.android/avd"); else entries+=("yes|$HOME/.android/avd"); fi
  for entry in "${entries[@]}"; do
    dir="${entry#*|}"
    real="$( (cd "$dir" 2>/dev/null && pwd -P) || echo "$dir")"
    duplicate=false
    for s in "${seen[@]+"${seen[@]}"}"; do [[ "$s" == "$real" ]] && duplicate=true; done
    [[ "$duplicate" == true ]] && continue
    seen+=("$real"); echo "$entry"
  done
}

join_dirs() { local out; out="$(printf '%s, ' "$@")"; echo "${out%, }"; }

find_avd_dir() {
  local entry searched dir all=()
  while IFS= read -r entry; do
    searched="${entry%%|*}"; dir="${entry#*|}"; all+=("$dir")
    [[ -f "$dir/$1.avd/config.ini" ]] || continue
    if [[ "$searched" != yes ]]; then
      die "The emulator does not search $dir. Set ANDROID_AVD_HOME=$dir (user environment variable) and open a new terminal."
    fi
    echo "$dir"; return 0
  done < <(avd_dirs)
  die "No AVD named '$1' in $(join_dirs "${all[@]}"). Run: ./android-dev.sh list"
}

tool() {
  [[ -n "${ANDROID_HOME:-}" ]] || die "ANDROID_HOME is not set. Point it at the Android SDK."
  local path="$ANDROID_HOME/$1"
  [[ -x "$path" ]] || die "$path is missing. Install it with the Android SDK Manager."
  echo "$path"
}

avd_tag() { # $1 = name, $2 = folder (optional)
  local dir="${2:-}"; [[ -n "$dir" ]] || dir="$(find_avd_dir "$1")"
  grep -m1 '^tag.id=' "$dir/$1.avd/config.ini" | cut -d= -f2 || echo unknown
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

boot_minutes() {
  local minutes="${POSTIT_BOOT_MINUTES:-20}"
  [[ "$minutes" =~ ^[0-9]+$ && "$minutes" -ge 1 ]] || die "POSTIT_BOOT_MINUTES must be a whole number of minutes, not '$minutes'."
  echo "$minutes"
}

log_path() { echo "${TMPDIR:-/tmp}/postit-emulator-$avd.log"; }

show_log_tail() { [[ -f "$(log_path)" ]] && tail -n 40 "$(log_path)" >&2 || true; }

assert_good_boot() { # $1 = serial (may be empty)
  local pattern line
  ((${#bad_boot_patterns[@]})) || return 0
  [[ -f "$(log_path)" ]] || return 0
  for pattern in "${bad_boot_patterns[@]}"; do
    line="$(grep -m1 -F -- "$pattern" "$(log_path)" || true)"
    [[ -n "$line" ]] || continue
    [[ -n "$1" ]] && "$adb" -s "$1" emu kill >/dev/null 2>&1 || true
    echo "$line" >&2
    die "Bad boot detected; run start again."
  done
}

wait_boot() {
  local minutes deadline
  minutes="$(boot_minutes)"; deadline=$((SECONDS + minutes * 60))
  "$adb" -s "$1" wait-for-device
  while ((SECONDS < deadline)); do
    [[ "$("$adb" -s "$1" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]] && return 0
    assert_good_boot "$1"
    sleep 1
  done
  show_log_tail
  die "$avd did not finish booting in $minutes minutes. Full log: $(log_path)"
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
  local started=$SECONDS
  nohup "$emulator" "${args[@]}" >"$(log_path)" 2>&1 &
  echo "Starting $avd (log: $(log_path)) ..." >&2
  for _ in $(seq 1 120); do
    serial="$(find_serial "$avd")"
    if [[ -n "$serial" ]]; then
      wait_boot "$serial"
      echo "Booted $avd in $((SECONDS - started)) s." >&2
      echo "$serial"; return 0
    fi
    assert_good_boot ""
    sleep 1
  done
  show_log_tail
  die "$avd did not show up in adb devices within 2 minutes. Full log: $(log_path)"
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
  if [[ "$force" != true ]]; then
    read -r -p "Wipe all data on $avd? Type yes to continue: " answer
    [[ "$answer" == "yes" ]] || { echo "Cancelled."; exit 0; }
  fi
  start_avd true >/dev/null
  echo "$avd was wiped and is running."
}

cmd_list() {
  local entry dir ini name tag note found=false all=()
  while IFS= read -r entry; do
    dir="${entry#*|}"; all+=("$dir")
    for ini in "$dir"/*.ini; do
      [[ -f "$ini" ]] || continue
      name="$(basename "$ini" .ini)"
      [[ -f "$dir/$name.avd/config.ini" ]] || continue
      found=true
      tag="$(avd_tag "$name" "$dir")"
      if [[ "$tag" == *playstore* ]]; then note="cannot be prepared (Google Play image)"; else note="can be prepared"; fi
      printf '%-28s %-32s %-40s %s\n' "$name" "$tag" "$note" "$dir"
    done
  done < <(avd_dirs)
  [[ "$found" == true ]] || echo "No AVDs in $(join_dirs "${all[@]}")."
}

usage() {
  cat <<'EOF'
Usage: ./android-dev.sh <command> [avd] [--force]

  list            List AVDs, whether they can be prepared, and the folder each is in.
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry) and print
                  how long the boot took.
  clear <avd>     Wipe the AVD's data. Asks first unless --force is given.
  reset <avd>     clear, then prepare.
  help            Show this text.

The AVD defaults to $POSTIT_AVD. Tools come from ANDROID_HOME.

AVDs are looked up in ANDROID_AVD_HOME, then ANDROID_USER_HOME/avd, then ~/.android/avd.
The script never sets ANDROID_AVD_HOME. The emulator searches only ANDROID_AVD_HOME (or
~/.android/avd when it is unset), so an AVD found anywhere else stops the script with a
request to set ANDROID_AVD_HOME.

The emulator console goes to ${TMPDIR:-/tmp}/postit-emulator-<avd>.log, rewritten on each
start. A boot that takes longer than POSTIT_BOOT_MINUTES (default 20) stops with the end of
that log.
EOF
}

case "$command" in
  help) usage; exit 0 ;;
  list) cmd_list; exit 0 ;;
  prepare|start|clear|reset) ;;
  *) usage; exit 1 ;;
esac
require_avd
find_avd_dir "$avd" >/dev/null
adb="$(tool platform-tools/adb)"
emulator="$(tool emulator/emulator)"
case "$command" in
  prepare) cmd_prepare ;;
  start) echo "$avd is running as $(start_avd false)." ;;
  clear) cmd_clear ;;
  reset) cmd_clear; cmd_prepare ;;
esac
