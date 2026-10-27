#!/usr/bin/env bash
# Create a BuildKit builder whose container is memory-capped, so building the
# Rust images cannot exhaust host RAM. Use with: BUILDX_BUILDER=ccal-capped docker compose build
set -euo pipefail
NAME="${1:-ccal-capped}"
MEM="${BUILD_MEM_MAX:-3g}"
if docker buildx inspect "$NAME" >/dev/null 2>&1; then
  echo "builder $NAME already exists"; exit 0
fi
docker buildx create --name "$NAME" --driver docker-container \
  --driver-opt "memory=$MEM" --driver-opt "memory-swap=$MEM" >/dev/null
docker buildx inspect --bootstrap "$NAME" >/dev/null
echo "created builder $NAME (memory cap $MEM)"
