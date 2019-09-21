#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

make -j"$(nproc)" -B svg 

if [[ -n $CI ]]; then
  # In CI confirm that no svgs need to be built 
  git diff --exit-code
fi
