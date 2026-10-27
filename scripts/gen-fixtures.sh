#!/usr/bin/env bash
# Regenerate diagnostics fixtures from the *real* judge toolchain.
# For every backend/crates/diagnostics/fixtures/<case>/main.c this compiles the
# file with the exact runner flags inside the runner image and stores the raw
# compiler stderr (JSON diagnostics + linker text) as stderr.txt.
# Usage: scripts/gen-fixtures.sh [image]   (default: cyculcal-runner:dev)
set -euo pipefail
IMAGE="${1:-cyculcal-runner:dev}"
DIR="$(cd "$(dirname "$0")/../backend/crates/diagnostics/fixtures" && pwd)"
docker run --rm --entrypoint /bin/sh -v "$DIR:/fixtures" -e LANG=C -e LC_ALL=C "$IMAGE" -c '
  gcc-12 --version | head -1 > /fixtures/TOOLCHAIN
  for d in /fixtures/*/; do
    cd "$d"
    rm -f /tmp/main
    # Same flags as backend/crates/runner/src/sandbox.rs COMPILE_FLAGS (output path aside).
    if /usr/bin/gcc-12 -std=c17 -O2 -pipe -Wall -Wextra -Wformat=2 -fdiagnostics-format=json \
        -fno-diagnostics-color -o /tmp/main main.c -lm 2> stderr.txt; then
      echo 0 > exit_code.txt
    else
      echo $? > exit_code.txt
    fi
    # Linker temp object names are random; normalise them for stable diffs.
    sed -i -E "s#/tmp/cc[A-Za-z0-9]+\.o#/tmp/ccXXXXXX.o#g" stderr.txt
  done
'
echo "regenerated fixtures with: $(cat "$DIR/TOOLCHAIN")"
