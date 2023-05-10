#!/usr/bin/env bash

set -e$ cargo install spl-feature-proposal-cli$ spl-feature-proposal address feature-proposal.json
Feature Id: HQ3baDfNU7WKCyWvtMYZmi51YPs7vhSiLn1ESYp3jhiA
Token Mint Address: ALvA7Lv9jbo8JFhxqnRpjWWuR3aD12uCb5KBJst4uc3d
Acceptance Token Address: AdqKm3mSJf8AtTWjfpA5ZbJszWQPcwyLA2XkRyLbf3Di

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
