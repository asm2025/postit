#!/usr/bin/env bash
# The encrypted vault archive, !ref/vault.7z. Twin: vault.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=vault
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
vault - pack, unpack, and check the encrypted vault archive, !ref/vault.7z.

USAGE
  ./vault.sh <command> [--force]

COMMANDS
  unpack    Extract !ref/vault.7z into !ref/vault/. Refuses when !ref/vault/
            already holds files, unless --force
  pack      Re-pack !ref/vault/ into !ref/vault.7z (AES-256, file names encrypted
            too), every environment included, .gitkeep left out. The old archive
            is replaced only when 7-Zip succeeds
  check     Compare the archive with !ref/vault/ file by file (SHA-256) and list
            the files that differ, by name only
  help      Show this help (also -h, --help, or no command)

  7-Zip asks for the password itself; it never becomes an argument.

OPTIONS
  --force   unpack: overwrite the files already in !ref/vault/

EXAMPLES
  ./vault.sh unpack                     First run on a new machine
  ./vault.sh check                      Did anything change since the last pack?
  ./vault.sh pack                       After editing a vault file; then commit
                                        !ref/vault.7z

EXIT CODES
  0  success; check: the archive matches !ref/vault/
  1  usage error, a refused guard, or a failure (a wrong password included)
  2  check: the archive and !ref/vault/ differ

MORE
  docs/scripts.md                       every operator script and the recipes
  README.md, "Environment files and secrets"
  Every committed archive stays readable with the password it was made with, so
  rotating a leaked secret means rotating the secret itself. pack keeps
  production/ until plan 03 G1 picks a secret store.
EOF
}

command="${1:-help}"
[ $# -gt 0 ] && shift

case "$command" in
    help | -h | --help)
        usage
        exit 0
        ;;
    unpack | pack | check) ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac

force=0
for arg in "$@"; do
    case "$arg" in
        --force) force=1 ;;
        *) die "unknown argument: $arg" ;;
    esac
done

vault="$(vault_dir)"
parent="$(dirname "$vault")"
name="$(basename "$vault")"
archive="${POSTIT_VAULT_ARCHIVE:-$POSTIT_ROOT/!ref/vault.7z}"

# POSTIT_7Z, else 7z or 7za on PATH, else the Windows installer's default location.
seven_zip() {
    if [ -n "${POSTIT_7Z:-}" ]; then
        command -v "$POSTIT_7Z" >/dev/null 2>&1 && printf '%s' "$POSTIT_7Z"
        return 0
    fi
    command -v 7z 2>/dev/null || command -v 7za 2>/dev/null ||
        { [ -f "C:/Program Files/7-Zip/7z.exe" ] && printf '%s' "C:/Program Files/7-Zip/7z.exe"; } || true
}

sz="$(seven_zip)"
[ -n "$sz" ] ||
    die "7-Zip is not installed: get it from https://www.7-zip.org/ (Linux: p7zip-full), or open !ref/vault.7z in the 7-Zip GUI as README.md describes"

# The password is 7-Zip's own prompt. POSTIT_7Z_TEST_PASSWORD exists for the tests only.
password_args=(-p)
[ -z "${POSTIT_7Z_TEST_PASSWORD:-}" ] || password_args=("-p$POSTIT_7Z_TEST_PASSWORD")

# The extracted tree: the archive's own vault/ folder, or its top when it has none.
tree_root() {
    if [ -d "$1/$name" ]; then printf '%s' "$1/$name"; else printf '%s' "$1"; fi
}

# Relative paths of the files under a directory, .gitkeep left out, sorted.
list_files() { (cd "$1" && find . -type f ! -name .gitkeep | sed 's#^\./##' | LC_ALL=C sort); }

do_unpack() {
    [ -f "$archive" ] || die "no archive at $archive"
    mkdir -p "$vault"
    if [ "$force" = 0 ] && [ -n "$(list_files "$vault")" ]; then
        die "$vault already holds files; run ./vault.sh check to compare, or unpack --force to overwrite them"
    fi
    # Extracted next to the files (inside the git-ignored vault folder), then moved in.
    # A global, so the EXIT trap still sees it.
    staging="$vault/.unpack.$$"
    trap 'rm -rf "$staging"' EXIT
    "$sz" x "${password_args[@]}" -o"$staging" "$archive" >/dev/null ||
        die "7-Zip could not extract $archive (a wrong password?); $vault is unchanged"
    cp -R "$(tree_root "$staging")"/. "$vault"/
    say "unpacked into $vault"
}

do_pack() {
    [ -d "$vault" ] && [ -n "$(list_files "$vault")" ] || die "nothing to pack: $vault holds no files"
    # A global, so the EXIT trap still sees it.
    pack_tmp="$archive.tmp.$$.7z"
    rm -f "$pack_tmp"
    trap 'rm -f "$pack_tmp"' EXIT
    # From the vault's parent, so every entry sits under vault/ in the archive.
    (cd "$parent" && "$sz" a "${password_args[@]}" -mhe=on '-xr!.gitkeep' "$pack_tmp" "$name" >/dev/null) ||
        die "7-Zip failed; $archive is unchanged"
    mv -f "$pack_tmp" "$archive"
    say "packed $vault into $archive; commit it"
}

do_check() {
    [ -f "$archive" ] || die "no archive at $archive"
    local a b differ=0 file
    # Outside the repository and owner-only (mktemp -d); a global, so the EXIT trap still
    # sees it, and in native form, so 7z.exe extracts where this script looks.
    check_tmp="$(native_path "$(mktemp -d)")"
    trap 'rm -rf "$check_tmp"' EXIT
    chmod 700 "$check_tmp" 2>/dev/null || true
    "$sz" x "${password_args[@]}" -o"$check_tmp" "$archive" >/dev/null ||
        die "7-Zip could not extract $archive (a wrong password?)"
    a="$(tree_root "$check_tmp")"
    b="$vault"
    while IFS= read -r file; do
        [ -n "$file" ] || continue
        if [ ! -f "$a/$file" ]; then
            echo "only in $vault: $file"
            differ=1
        elif [ ! -f "$b/$file" ]; then
            echo "only in the archive: $file"
            differ=1
        elif [ "$(sha256sum <"$a/$file" | cut -d' ' -f1)" != "$(sha256sum <"$b/$file" | cut -d' ' -f1)" ]; then
            echo "changed: $file"
            differ=1
        fi
    done < <({ list_files "$a"; list_files "$b"; } | LC_ALL=C sort -u)
    if [ "$differ" = 1 ]; then
        say "the archive and $vault differ; ./vault.sh pack to update the archive"
        exit 2
    fi
    say "the archive matches $vault"
}

case "$command" in
    unpack) do_unpack ;;
    pack) do_pack ;;
    check) do_check ;;
esac
