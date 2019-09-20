#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

make -j"$(nproc)" svg 

[[ -z "$CI" ]] && git diff --exit-code