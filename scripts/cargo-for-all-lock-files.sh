#!/usr/bin/env bash

set -ex

for lock_file in $(git ls-files :**/Cargo.lock)
do
  (
    cd "$(dirname "$lock_file")"
    cargo "$@"
  )
done
