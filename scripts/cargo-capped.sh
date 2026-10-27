#!/usr/bin/env bash
# Run cargo with a hard memory cap so rustc cannot exhaust the host's RAM.
# Usage: scripts/cargo-capped.sh build | test | clippy ...
# Override with CARGO_MEM_MAX (default 3G) and CARGO_BUILD_JOBS (default 2).
set -euo pipefail
cd "$(dirname "$0")/../backend"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
exec systemd-run --user --scope --quiet --collect \
  -p MemoryMax="${CARGO_MEM_MAX:-3G}" -p MemorySwapMax=0 \
  --setenv=CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" \
  cargo "$@"
