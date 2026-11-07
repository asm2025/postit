#!/usr/bin/env bash
# Shared by the root operator scripts (compose, db, vault, doctor, cert). Sourced, never
# run. The twin is common.ps1; keep the two in step.
#
# A root script sets POSTIT_SCRIPT_NAME, then sources this file.

# `pwd -W` (Git Bash / MSYS only) yields D:/... instead of /d/..., which a Windows
# docker.exe would otherwise read as a path on the current drive (D:\d\...).
POSTIT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && { pwd -W 2>/dev/null || pwd; })"

# The repository root, absolute, in Windows form under Git Bash.
repo_root() { printf '%s' "$POSTIT_ROOT"; }

# Prints "<script>: <message>" to stderr and exits 1.
die() {
    echo "${POSTIT_SCRIPT_NAME:-postit}: $*" >&2
    exit 1
}

# Prints "<script>: <message>" to stdout.
say() { echo "${POSTIT_SCRIPT_NAME:-postit}: $*"; }

# An absolute path in the form native programs read: D:/... under Git Bash, where /tmp/x
# would reach a Windows program as a path on the current drive. Unchanged elsewhere.
native_path() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m -a "$1"; else printf '%s' "$1"; fi
}

# Prints the word when it names an environment, else nothing.
parse_environment() {
    case "${1:-}" in
        development | qa | production) printf '%s' "$1" ;;
    esac
}

# KEY's value in a dotenv file, read as data (a vault file is never sourced): the last
# matching line, a trailing CR dropped, one pair of surrounding single or double quotes
# removed, matching postit-config and Compose. Empty when the file or key is missing.
env_value() {
    local file="$1" key="$2" line value
    [ -f "$file" ] || return 0
    line="$(grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 || true)"
    [ -n "$line" ] || return 0
    value="${line#*=}"
    value="${value%$'\r'}"
    case "$value" in
        \'*\') value="${value:1:${#value}-2}" ;;
        \"*\") value="${value:1:${#value}-2}" ;;
    esac
    printf '%s' "$value"
}

# The vault files an environment needs, one per project.
vault_files() {
    case "$1" in
        development) echo postgres.env postit.env zitadel.env ;;
        *) echo postgres.env postit.env ;;
    esac
}

# The extracted vault: POSTIT_VAULT_DIR, or !ref/vault.
vault_dir() { printf '%s' "${POSTIT_VAULT_DIR:-$POSTIT_ROOT/!ref/vault}"; }

# Runs docker (POSTIT_DOCKER overrides, for tests). Every script calls Docker through it.
docker_cmd() { "${POSTIT_DOCKER:-docker}" "$@"; }

# The compose arguments for an environment, one per line: the base file plus exactly one
# environment file, always. Read with `mapfile -t compose < <(compose_args "$env")`.
compose_args() {
    printf '%s\n' compose \
        -f "$POSTIT_ROOT/docker/docker-compose.yml" \
        -f "$POSTIT_ROOT/docker/docker-compose.$1.yml"
}

# confirm <expected> <prompt>: prints the prompt to stderr, reads one line from standard
# input, and succeeds only when it is exactly <expected>. End of input is a "no".
confirm() {
    local expected="$1" prompt="$2" typed
    printf '%s' "$prompt" >&2
    # A last line without a newline still counts; no input at all is a "no".
    if ! IFS= read -r typed; then
        echo >&2
        [ -n "$typed" ] || return 1
    fi
    [ "${typed%$'\r'}" = "$expected" ]
}

# The environment the running postit-postgres was started for, read from the compose
# config-file label (".../docker-compose.<environment>.yml"). Empty when there is no such
# container or no label. Every environment shares the project, the container names, and
# the database volume, so a command that acts on the database checks this first.
running_environment() {
    local files
    files="$(docker_cmd inspect postit-postgres \
        --format '{{index .Config.Labels "com.docker.compose.project.config_files"}}' 2>/dev/null || true)"
    printf '%s' "$files" | tr ',' '\n' |
        sed -nE 's#.*docker-compose\.(development|qa|production)\.yml\r?$#\1#p' | tail -n 1
}
