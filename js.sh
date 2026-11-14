#!/usr/bin/env bash
# The JavaScript tooling of the web client (web/): install, update, build, and the quality
# gate, always through pnpm. Twin: js.ps1.
set -euo pipefail
POSTIT_SCRIPT_NAME=js
# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/scripts/lib/common.sh"

usage() {
    cat <<'EOF'
js - manage the JavaScript packages of the web client (web/) with pnpm.

USAGE
  ./js.sh <command> [options]

COMMANDS
  install     Install the packages (pnpm install --prefer-frozen-lockfile)
  update      Update the packages within the ranges in package.json
  build       Build the web client (pnpm run build)
  check       The web quality gate: typecheck, lint, format:check, test, build;
              stops at the first failure
  clean       Remove web/node_modules and web/dist (the lockfile stays)
  list        List the projects with a package.json
  audit       Audit the packages for known vulnerabilities (pnpm audit)
  outdated    List packages with a newer version (pnpm outdated)
  help        Show this help (also -h, --help)

OPTIONS
  -c          CI mode for install: --frozen-lockfile, so a stale lockfile fails.
              On by default when CI, GITHUB_ACTIONS, GITLAB_CI, JENKINS_HOME,
              TF_BUILD, or CIRCLECI is set
  -l          For update: also cross major versions (rewrites package.json
              ranges). Same as --latest

  POSTIT_PNPM=<path>   use this pnpm executable (tests use it)

  pnpm only, never npm. On Windows Git Bash the pnpm shim can fail: run
  ./js.ps1 from PowerShell instead.

EXAMPLES
  ./js.sh install                       Restore the packages
  ./js.sh install -c                    Same, failing on a stale lockfile
  ./js.sh update -l                     Move every package to its latest major
  ./js.sh check                         The gate CI runs for the web client

EXIT CODES
  0  success
  1  usage error, pnpm missing, or a step failed (pnpm's own code when it fails)

MORE
  docs/scripts.md                       every operator script and the first-run order
  web/CLAUDE.md                         the web client's commands
EOF
}

command="${1:-help}"
case "$command" in
    help | -h | --help)
        usage
        exit 0
        ;;
    install | update | build | check | clean | list | audit | outdated) shift ;;
    *)
        usage >&2
        die "unknown command: $command"
        ;;
esac

ci_mode=0
latest=0
for arg in "$@"; do
    case "$arg" in
        -c) ci_mode=1 ;;
        -l | --latest) latest=1 ;;
        *)
            usage >&2
            die "unknown option: $arg"
            ;;
    esac
done
if [ "$ci_mode" = 0 ] && { [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${GITLAB_CI:-}" ] ||
    [ -n "${JENKINS_HOME:-}" ] || [ -n "${TF_BUILD:-}" ] || [ -n "${CIRCLECI:-}" ]; }; then
    ci_mode=1
fi

projects=(web)
pnpm="${POSTIT_PNPM:-pnpm}"

# Runs pnpm in the project folder, exiting with its code when it fails.
run_pnpm() {
    local code=0
    (cd "$POSTIT_ROOT/$project" && "$pnpm" "$@") || code=$?
    [ "$code" -eq 0 ] || exit "$code"
}

if [ "$command" != list ] && ! command -v "$pnpm" >/dev/null 2>&1; then
    die "pnpm not found; install it (corepack enable, or https://pnpm.io/installation) and re-run"
fi

for project in "${projects[@]}"; do
    [ -f "$POSTIT_ROOT/$project/package.json" ] || continue
    case "$command" in
        list)
            echo "$project"
            echo "  Path: $POSTIT_ROOT/$project"
            ;;
        install)
            if [ "$ci_mode" = 1 ]; then
                say "$project: installing (CI mode, frozen lockfile)"
                run_pnpm install --frozen-lockfile
            else
                say "$project: installing"
                run_pnpm install --prefer-frozen-lockfile
            fi
            ;;
        update)
            if [ "$latest" = 1 ]; then
                say "$project: updating to the latest versions (majors may change)"
                run_pnpm update --latest
            else
                say "$project: updating within the ranges in package.json (-l crosses majors)"
                run_pnpm update
            fi
            ;;
        build)
            say "$project: building"
            run_pnpm run build
            ;;
        check)
            for step in typecheck lint format:check test build; do
                say "$project: $step"
                run_pnpm run "$step"
            done
            ;;
        clean)
            for path in node_modules dist; do
                if [ -e "$POSTIT_ROOT/$project/$path" ]; then
                    rm -rf "${POSTIT_ROOT:?}/$project/$path"
                    say "$project: removed $path"
                fi
            done
            ;;
        audit)
            run_pnpm audit
            ;;
        outdated)
            # pnpm outdated exits 1 when anything is outdated; that is a finding, not a failure.
            (cd "$POSTIT_ROOT/$project" && "$pnpm" outdated) || true
            ;;
    esac
done
exit 0
