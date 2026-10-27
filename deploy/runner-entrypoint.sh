#!/bin/sh
# Prepare a delegated cgroup v2 subtree for isolate inside this (privileged,
# cgroupns=private) container, then exec the runner.
set -eu

CG=/sys/fs/cgroup
if [ ! -f "$CG/cgroup.controllers" ]; then
  echo "judge-runner: cgroup v2 is required (no $CG/cgroup.controllers)" >&2
  exit 1
fi

# cgroup v2 "no internal processes" rule: move ourselves out of the root
# before enabling controllers for children.
mkdir -p "$CG/init" "$CG/isolate"
for pid in $(cat "$CG/cgroup.procs"); do
  echo "$pid" > "$CG/init/cgroup.procs" 2>/dev/null || true
done
for c in cpu memory pids; do
  echo "+$c" > "$CG/cgroup.subtree_control"
  echo "+$c" > "$CG/isolate/cgroup.subtree_control"
done
# Never let sandboxed programs push memory to swap.
echo 0 > "$CG/isolate/memory.swap.max" 2>/dev/null || true

mkdir -p /run/isolate/locks
echo "$CG/isolate" > /run/isolate/cgroup

isolate-check-environment --quiet >/tmp/isolate-env.txt 2>&1 || true
if [ -s /tmp/isolate-env.txt ]; then
  echo "judge-runner: isolate-check-environment notes (non-fatal):" >&2
  cat /tmp/isolate-env.txt >&2
fi

exec "$@"
