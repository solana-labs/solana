#!/usr/bin/env bash

set -e

if [[ -n $_TARGET_LOCK_FILES ]]; then
  files="$_TARGET_LOCK_FILES"
else
  files="$(git ls-files :**/Cargo.lock)"
fi

for lock_file in $files
do
  (
    set -x
    cd "$(dirname "$lock_file")"
    cargo "$@"
  )
done
