#!/usr/bin/env bash
# Fake host tool (sqlx, cargo) for the operator script tests: logs the call to
# POSTIT_FAKE_DOCKER_LOG as "<tool> <args> db=<database> in=<directory>", naming only the
# database in DATABASE_URL (never the URL, which holds a password) and the directory it
# ran in. FAKE_<TOOL>_FAIL=1 makes it exit 1. Mirrors tool.ps1.
tool="$1"
shift
db="${DATABASE_URL:-}"
db="${db##*/}"
db="${db%%\?*}"
[ -z "${POSTIT_FAKE_DOCKER_LOG:-}" ] ||
    printf '%s %s db=%s in=%s offline=%s\n' "$tool" "$*" "$db" "$(basename "$PWD")" "${SQLX_OFFLINE:-}" >>"$POSTIT_FAKE_DOCKER_LOG"
fail="FAKE_${tool^^}_FAIL"
[ "${!fail:-}" != 1 ] || exit 1
exit 0
