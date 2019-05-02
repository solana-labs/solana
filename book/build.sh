#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

make -j"$(nproc)"
