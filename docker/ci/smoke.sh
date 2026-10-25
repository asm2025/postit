#!/usr/bin/env bash
# Waits for every probe of the smoke stack to answer 200, or fails with the containers'
# logs. Run after `docker compose -f docker/ci/docker-compose.smoke.yml up -d`.
set -euo pipefail

compose=(docker compose -f "$(dirname "${BASH_SOURCE[0]}")/docker-compose.smoke.yml")
urls=(
    http://127.0.0.1:8080/health
    http://127.0.0.1:8080/ready
    http://127.0.0.1:8081/health
    http://127.0.0.1:8081/ready
    http://127.0.0.1:8082/config.json
)

for url in "${urls[@]}"; do
    for _ in $(seq 1 90); do
        if curl -fsS "$url" > /dev/null; then
            echo "ok   $url"
            continue 2
        fi
        sleep 2
    done
    echo "smoke: $url never answered 200" >&2
    "${compose[@]}" ps >&2
    "${compose[@]}" logs --no-color >&2
    exit 1
done

# /config.json must carry this deployment's runtime config (the compose file sets
# API_BASE_URL and POSTIT_ENV), not a default baked into the build.
body="$(curl -fsS http://127.0.0.1:8082/config.json)"
case "$body" in
    *'"API_BASE_URL":"http://127.0.0.1:8080"'*) ;;
    *) echo "smoke: /config.json has the wrong API_BASE_URL: $body" >&2; exit 1 ;;
esac
case "$body" in
    *'"ENV":"development"'*) ;;
    *) echo "smoke: /config.json has the wrong ENV: $body" >&2; exit 1 ;;
esac
# The CI image is built with the mock agent, and startup logs the agents it registered.
logs="$("${compose[@]}" logs --no-color)"
# `--no-color` drops only compose's prefix colors; the API's pretty log carries its own ANSI
# codes, so strip them. Match per line, in any field order: the one line must name both. The
# mock's platform ids are `mock` and `mock_direct`; match the one no other id contains.
esc=$'\033'
plain="$(sed -E "s/${esc}\[[0-9;]*m//g" <<<"$logs")"
matches="$(grep -F 'registered agents' <<<"$plain" || true)"
if [[ "$matches" != *'"mock_direct"'* ]]; then
    echo "smoke: the API log has no 'registered agents' line naming the mock_direct platform" >&2; echo "$logs" >&2; exit 1
fi
echo "smoke: all probes passed"
