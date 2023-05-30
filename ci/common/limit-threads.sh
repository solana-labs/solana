#!/usr/bin/env bash

set -e

# limit jobs to 4gb/thread
if [[ -f "/proc/meminfo" ]]; then
  JOBS=$(grep MemTotal /proc/meminfo | awk '{printf "%.0f", ($2 / (4 * 1024 * 1024))}')
else
  JOBS=$(sysctl hw.memsize | awk '{printf "%.0f", ($2 / (4 * 1024**3))}')
fi

NPROC=$(nproc)
JOBS=$((JOBS > NPROC ? NPROC : JOBS))

export NPROC
export JOBS
