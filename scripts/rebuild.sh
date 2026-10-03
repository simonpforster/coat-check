#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

services=("$@")

echo "==> Building..."
if [ ${#services[@]} -eq 0 ]; then
    docker compose build --parallel
else
    docker compose build "${services[@]}"
fi

echo "==> Restarting..."
if [ ${#services[@]} -eq 0 ]; then
    docker compose up -d --force-recreate
else
    docker compose up -d --force-recreate "${services[@]}"
fi

echo "==> Running containers:"
docker compose ps
