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

# /config.json must describe this deployment, not a dev default baked into the build.
curl -fsS http://127.0.0.1:8082/config.json | grep -q '"ENV"' ||
    { echo "smoke: /config.json has no ENV" >&2; exit 1; }
echo "smoke: all probes passed"
