#!/usr/bin/env bash
# Starts the PostgreSQL and the headless browser the tests use, on ports that do not collide with
# other stacks on a development machine (25435 and 29150; pacod-gw's tests use 25433, pacod-id's
# 25434, pacod-benchmark's 25436). Safe to run again: a running container is left alone. The
# password is a test value and the ports are bound to 127.0.0.1. Each test makes a database of its
# own.
set -euo pipefail

# Not agg-*: that prefix is the stack's own services, and pacod-demo's preflight takes a running
# agg-* container for another stack (as pacod-gw's pgw-test-* and pacod-id's pid-test-pg avoid theirs).
start() {
  local name="$1"
  shift
  if docker ps --format '{{.Names}}' | grep -qx "$name"; then
    return
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    docker start "$name" >/dev/null
  else
    docker run -d --name "$name" "$@" >/dev/null
  fi
}

start pagg-test-pg --shm-size=256m -e POSTGRES_PASSWORD=test -p 127.0.0.1:25435:5432 postgres:18.6-alpine3.24

# The scraper's browser, built from the working tree and run as deploy/docker-compose.yml runs it.
# Its tests give each page a proxy in the test process, which the browser reaches as
# host.docker.internal.
root="$(cd "$(dirname "$0")/.." && pwd)"
# Without a provenance attestation, which would give every build, even a cached one, a new id.
docker build -q --provenance=false -f "$root/deploy/docker/browser.Dockerfile" -t pacod-agg-browser:test "$root" \
  >/dev/null
# A container of another image, such as a build of an earlier Chromium, is replaced, not left
# running.
if [ "$(docker inspect -f '{{.Image}}' pagg-test-browser 2>/dev/null)" != \
  "$(docker image inspect -f '{{.Id}}' pacod-agg-browser:test)" ]; then
  docker rm -f pagg-test-browser >/dev/null 2>&1 || true
fi
start pagg-test-browser --shm-size=256m --user 65534:65534 -e HOME=/tmp --read-only --tmpfs /tmp \
  --cap-drop ALL --security-opt no-new-privileges --add-host host.docker.internal:host-gateway \
  -p 127.0.0.1:29150:9222 pacod-agg-browser:test

for _ in $(seq 1 60); do
  if docker exec pagg-test-pg pg_isready -q -h 127.0.0.1 -U postgres 2>/dev/null &&
    curl -fsS -o /dev/null -H 'Host: 127.0.0.1' http://127.0.0.1:29150/json/version 2>/dev/null; then
    echo "test services are ready: postgres on 25435, browser on 29150"
    exit 0
  fi
  sleep 1
done
echo "the test services did not become ready within 60 s" >&2
exit 1
