#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Tearing down..."
docker compose down -v


services=("$@")

echo "==> Building..."
if [ ${#services[@]} -eq 0 ]; then
    docker compose build --parallel
else
    docker compose build "${services[@]}"
fi

echo "==> Restarting..."
if [ ${#services[@]} -eq 0 ]; then
    docker compose up -d --force-recreate --wait
else
    docker compose up -d --force-recreate --wait "${services[@]}"
fi

echo "==> Running containers:"
docker compose ps
