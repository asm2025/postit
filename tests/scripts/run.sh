#!/usr/bin/env bash
# Runs every operator script test file (tests/scripts/*.sh except this one and lib.sh).
# Exit 1 when any file fails. Each file runs with empty standard input, so a script that
# asks a question gets "no" instead of waiting.
set -uo pipefail

dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failed=()
for file in "$dir"/*.sh; do
    case "$(basename "$file")" in lib.sh | run.sh) continue ;; esac
    echo "== $(basename "$file")"
    bash "$file" </dev/null || failed+=("$(basename "$file")")
done
if [ ${#failed[@]} -gt 0 ]; then
    echo "failed: ${failed[*]}" >&2
    exit 1
fi
echo "all script tests passed"
