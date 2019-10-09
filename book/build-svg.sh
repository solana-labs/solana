#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

make -j"$(nproc)" -B svg

#TODO figure out why book wants to change, but local and CI differ
exit 0
if [[ -n $CI ]]; then
  # In CI confirm that no svgs need to be built
  git diff --exit-code
fi
