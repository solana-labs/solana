#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

channel=${1:-stable}
if ! ./version-check.sh "$channel"; then
  rustup install "$channel"
  rustup default "$channel"
  ./version-check.sh "$channel"
fi
