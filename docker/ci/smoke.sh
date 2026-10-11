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
echo "smoke: all probes passed"
