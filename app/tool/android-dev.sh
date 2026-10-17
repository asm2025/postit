#!/usr/bin/env bash
# Prepares an Android emulator for postit development. See android-dev.ps1 for details;
# both scripts offer the same commands.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
ca_path="$repo_root/docker/shared/nginx/certs/postit-dev-ca.crt"
hosts_line='10.0.2.2 postit.local'
cli_base_url=https://dl.google.com/android/cli/latest
command="${1:-help}"
avd="${2:-}"
[[ "$avd" == --* ]] && avd=""
avd="${avd:-${POSTIT_AVD:-}}"
force=false
image='system-images;android-35;google_apis;x86_64'
device_profile=pixel_8
ram_mb=4096
cores=4
data_size=8G
for arg in "$@"; do
  case "$arg" in
    --force) force=true ;;
    --image=*) image="${arg#*=}" ;;
    --device=*) device_profile="${arg#*=}" ;;
    --ram=*) ram_mb="${arg#*=}" ;;
    --cores=*) cores="${arg#*=}" ;;
    --data=*) data_size="${arg#*=}" ;;
  esac
done

# Lines that mean this boot will never finish; add the message when it is seen again.
bad_boot_patterns=()

die() { echo "error: $*" >&2; exit 1; }

# Folders that may hold AVDs, in lookup order, one "searched|path" per line. The emulator
# searches only ANDROID_AVD_HOME, or ~/.android/avd when that is unset, so the script reads
# these and never sets the variable. A folder named twice keeps its first entry, so the
# searched folders come first: ANDROID_USER_HOME=~/.android must not hide ~/.android/avd.
avd_dirs() {
  local entries=() entry dir seen=() s real duplicate
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then entries+=("yes|$ANDROID_AVD_HOME"); fi
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then entries+=("no|$HOME/.android/avd"); else entries+=("yes|$HOME/.android/avd"); fi
  if [[ -n "${ANDROID_USER_HOME:-}" ]]; then entries+=("no|$ANDROID_USER_HOME/avd"); fi
  for entry in "${entries[@]}"; do
    dir="${entry#*|}"
    real="$( (cd "$dir" 2>/dev/null && pwd -P) || echo "$dir")"
    duplicate=false
    for s in "${seen[@]+"${seen[@]}"}"; do [[ "$s" == "$real" ]] && duplicate=true; done
    [[ "$duplicate" == true ]] && continue
    seen+=("$real"); echo "$entry"
  done
}

# avdmanager writes to ANDROID_AVD_HOME, else ANDROID_USER_HOME/avd, else ~/.android/avd. Stop
# before creating anything there when the emulator would not find it.
assert_create_target() {
  local target entry real
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then target="$ANDROID_AVD_HOME"
  elif [[ -n "${ANDROID_USER_HOME:-}" ]]; then target="$ANDROID_USER_HOME/avd"
  else target="$HOME/.android/avd"; fi
  real="$( (cd "$target" 2>/dev/null && pwd -P) || echo "$target")"
  while IFS= read -r entry; do
    [[ "$( (cd "${entry#*|}" 2>/dev/null && pwd -P) || echo "${entry#*|}")" == "$real" ]] || continue
    [[ "${entry%%|*}" == yes ]] && return 0
    break
  done < <(avd_dirs)
  die "The emulator does not search $target. Set ANDROID_AVD_HOME=$target (user environment variable) and open a new terminal."
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
  # The cmdline-tools are .bat scripts on Windows.
  if [[ ! -x "$path" && -f "$path.bat" ]]; then path="$path.bat"; fi
  [[ -x "$path" || -f "$path" ]] || die "$path is missing. Install it with the Android SDK Manager."
  echo "$path"
}

# Runs Google's installer for this platform (the commands the android-cli skill documents).
install_android_cli() {
  local os arch installer
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=darwin ;;
    MINGW* | MSYS* | CYGWIN*)
      installer="${TMPDIR:-/tmp}/android-cli-install.cmd"
      curl -fsSL "$cli_base_url/windows_x86_64/install.cmd" -o "$installer" || die "Downloading the Android CLI installer failed."
      cmd.exe //c "$(cygpath -w "$installer")" || die "The Android CLI installer failed."
      return 0 ;;
    *) die "There is no Android CLI installer for $(uname -s)." ;;
  esac
  case "$(uname -m)" in arm64 | aarch64) arch=arm64 ;; *) arch=x86_64 ;; esac
  curl -fsSL "$cli_base_url/${os}_$arch/install.sh" | bash || die "The Android CLI installer failed."
}

android_cli() {
  if command -v android >/dev/null; then command -v android; return 0; fi
  local answer
  read -r -p "The Android CLI (android) is not on PATH. Install it now? Type yes to continue: " answer
  [[ "$answer" == "yes" ]] || die "./android-dev.sh $command needs the Android CLI."
  install_android_cli >&2
  command -v android >/dev/null || die "The Android CLI is installed but not on PATH yet. Open a new terminal and run the command again."
  command -v android
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

wait_boot() { # $1 = serial, $2 = deadline in $SECONDS (optional)
  local minutes deadline
  minutes="$(boot_minutes)"; deadline="${2:-$((SECONDS + minutes * 60))}"
  "$adb" -s "$1" wait-for-device
  while ((SECONDS < deadline)); do
    [[ "$("$adb" -s "$1" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]] && return 0
    assert_good_boot "$1"
    sleep 1
  done
  show_log_tail
  die "$avd did not finish booting in $minutes minutes. Full log: $(log_path)"
}

# The emulator keeps running after adb loses the device while it saves the quick boot
# snapshot, and a second launch of the same AVD fails until it exits.
avd_process_running() {
  local pattern="(-avd +|@)$avd( |\"|$)"
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      powershell.exe -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name like 'emulator%' or Name like 'qemu-system%'\" | ForEach-Object CommandLine" |
        tr -d '\r' | grep -Eq -- "$pattern" ;;
    *) ps -eo args= | grep -E -- '^[^ ]*(emulator|qemu-system)[^ ]* ' | grep -Eq -- "$pattern" ;;
  esac
}

# Helpers the emulator starts (netsimd, crashpad_handler) can hold the console log a little
# longer, and the next start cannot rewrite it until they let go. Truncating it is the test.
log_free() { (: >"$(log_path)") 2>/dev/null; }

wait_avd_exit() {
  for _ in $(seq 1 60); do
    if ! avd_process_running && log_free; then return 0; fi
    sleep 1
  done
  die "The emulator for $avd (or its log $(log_path)) was still in use 60 s after it left adb."
}

start_avd() { # $1 = wipe flag (true|false); prints the serial
  local serial; serial="$(find_serial "$avd")"
  if [[ -n "$serial" ]]; then
    if [[ "$1" != true ]]; then echo "$serial"; return 0; fi
    "$adb" -s "$serial" emu kill >/dev/null
    while [[ -n "$(find_serial "$avd")" ]]; do sleep 1; done
  fi
  wait_avd_exit
  local args=(-avd "$avd" -writable-system)
  [[ "$1" == true ]] && args+=(-wipe-data)
  local started=$SECONDS
  nohup "$emulator" "${args[@]}" >"$(log_path)" 2>&1 &
  echo "Starting $avd (log: $(log_path)) ..." >&2
  # One deadline covers both steps: a snapshot start can stay offline in adb for minutes.
  local minutes deadline
  minutes="$(boot_minutes)"; deadline=$((started + minutes * 60))
  while ((SECONDS < deadline)); do
    serial="$(find_serial "$avd")"
    if [[ -n "$serial" ]]; then
      wait_boot "$serial" "$deadline"
      echo "Booted $avd in $((SECONDS - started)) s." >&2
      echo "$serial"; return 0
    fi
    assert_good_boot ""
    sleep 1
  done
  show_log_tail
  die "$avd did not come online in adb within $minutes minutes. Full log: $(log_path)"
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

cmd_stop() {
  [[ -n "$(find_serial "$avd")" ]] || { echo "$avd is not running."; return 0; }
  "$cli" emulator stop "$avd" || die "android emulator stop $avd failed."
  for _ in $(seq 1 60); do
    [[ -n "$(find_serial "$avd")" ]] || { wait_avd_exit; echo "$avd stopped."; return 0; }
    sleep 1
  done
  die "$avd was still running 60 s after android emulator stop."
}

# Saves a screenshot and the layout tree, for checking the app on the emulator.
cmd_ui() {
  local device_args=() serial base="${TMPDIR:-/tmp}/postit-ui"
  # Git Bash hands /tmp to Windows programs as <drive>:\tmp; give them the real folder.
  if command -v cygpath >/dev/null; then base="$(cygpath -w "$base")"; fi
  if [[ -n "$avd" ]]; then
    serial="$(find_serial "$avd")"
    [[ -n "$serial" ]] || die "$avd is not running. Run: ./android-dev.sh start $avd"
    device_args=("--device=$serial")
  fi
  "$cli" screen capture "${device_args[@]+"${device_args[@]}"}" "--output=$base.png" || die "android screen capture failed."
  "$cli" layout "${device_args[@]+"${device_args[@]}"}" --pretty "--output=$base.json" || die "android layout failed."
  echo "Screenshot: $base.png"
  echo "Layout: $base.json"
}

# Replaces the given keys in an AVD config.ini, adding the ones it lacks. $1 = file, then key=value.
set_ini() {
  local file="$1" pair keys=() tmp
  shift
  for pair in "$@"; do keys+=("${pair%%=*}"); done
  tmp="$(mktemp)"
  awk -v keys="${keys[*]}" 'BEGIN { n = split(keys, k, " "); for (i = 1; i <= n; i++) drop[k[i]] = 1 }
    { split($0, kv, "="); if (!(kv[1] in drop)) print }' "$file" >"$tmp"
  printf '%s\n' "$@" >>"$tmp"
  mv "$tmp" "$file"
}

cmd_create() {
  local entry dir output accel
  [[ "$ram_mb" =~ ^[0-9]+$ ]] || die "--ram must be a whole number of megabytes, not '$ram_mb'."
  [[ "$cores" =~ ^[0-9]+$ ]] || die "--cores must be a whole number, not '$cores'."
  while IFS= read -r entry; do
    dir="${entry#*|}"
    [[ ! -e "$dir/$avd.avd" ]] || die "An AVD named '$avd' already exists in $dir. Pick another name or delete it first."
  done < <(avd_dirs)
  [[ "$image" != *playstore* ]] || die "$image is a Google Play image, which cannot be prepared. Use a Google APIs image."
  assert_create_target
  local package="${image//;//}"
  if [[ ! -d "$ANDROID_HOME/$package" ]]; then
    # sdkmanager.bat now forwards to the Android CLI, which needs '/' instead of ';'.
    local cli; cli="$(android_cli)"
    echo "Installing $package ..."
    "$cli" sdk install "$package" || die "android sdk install $package failed."
  fi
  # avdmanager asks whether to create a custom hardware profile; the answer is no.
  if ! output="$(echo no | "$avdmanager" create avd --name "$avd" --package "$image" --device "$device_profile" 2>&1)"; then
    echo "$output" >&2
    die "avdmanager could not create $avd."
  fi
  dir="$(find_avd_dir "$avd")"
  # Speed: the host GPU, enough RAM and cores, and quick boot. Hardware the app does not use
  # (cameras, microphone) and the device frame are off.
  set_ini "$dir/$avd.avd/config.ini" \
    hw.gpu.enabled=yes hw.gpu.mode=host "hw.ramSize=$ram_mb" vm.heapSize=512M "hw.cpu.ncore=$cores" \
    "disk.dataPartition.size=$data_size" hw.keyboard=yes hw.camera.back=none hw.camera.front=none \
    hw.audioInput=no showDeviceFrame=no fastboot.forceColdBoot=no fastboot.forceFastBoot=yes
  accel="$("$emulator" -accel-check 2>&1 || true)"
  if ! grep -q 'is installed and usable' <<<"$accel"; then
    echo "$accel" >&2
    echo "warning: hardware acceleration is not usable, so the emulator will be slow. Enable KVM (Linux), WHPX (Windows) or check Hypervisor.framework (macOS)." >&2
  fi
  echo "Created $avd ($image, $device_profile, $ram_mb MB RAM, $cores cores, host GPU)."
  echo "Next: ./android-dev.sh prepare $avd"
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
Usage: ./android-dev.sh <command> [avd] [--force] [create options]

  list            List AVDs, whether they can be prepared, and the folder each is in.
  create <avd>    Create an AVD tuned for speed: Google APIs image (rootable, so prepare
                  works), host GPU, 4096 MB RAM, 4 cores, 512 MB heap, 8G data partition,
                  quick boot, no cameras, microphone or device frame. Installs the image
                  first when it is missing. Options: --image= (default
                  'system-images;android-35;google_apis;x86_64'), --device= (pixel_8),
                  --ram= (MB), --cores=, --data= (for example 8G).
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry) and print
                  how long the boot took.
  stop <avd>      Shut the AVD down (android emulator stop).
  clear <avd>     Wipe the AVD's data. Asks first unless --force is given.
  reset <avd>     clear, then prepare.
  ui [avd]        Save a screenshot and the layout tree to ${TMPDIR:-/tmp}/postit-ui.png and
                  .json (android screen capture, android layout). Without an AVD, the only
                  connected device is used.
  help            Show this text.

The AVD defaults to $POSTIT_AVD. SDK tools come from ANDROID_HOME. stop and ui need the
Android CLI (android) on PATH and offer to install it when it is missing.

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
  create|prepare|start|stop|clear|reset) require_avd ;;
  ui) ;;
  *) usage; exit 1 ;;
esac
adb="$(tool platform-tools/adb)"
emulator="$(tool emulator/emulator)"
case "$command" in
  create)
    avdmanager="$(tool cmdline-tools/latest/bin/avdmanager)"
    cmd_create
    exit 0 ;;
  stop|ui) cli="$(android_cli)" ;;
esac
[[ "$command" == ui ]] || find_avd_dir "$avd" >/dev/null
case "$command" in
  prepare) cmd_prepare ;;
  start) serial="$(start_avd false)"; echo "$avd is running as $serial." ;;
  stop) cmd_stop ;;
  ui) cmd_ui ;;
  clear) cmd_clear ;;
  reset) cmd_clear; cmd_prepare ;;
esac
