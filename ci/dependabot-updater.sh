#!/usr/bin/env bash

set -ex

commit_range="$(git merge-base HEAD origin/master)..HEAD"
parsed_update_args="$(
  git log "$commit_range" --author "dependabot-preview" --oneline -n1 |
    grep -o 'Bump.*$' |
    sed -r 's/Bump ([^ ]+) from [^ ]+ to ([^ ]+)/-p \1 --precise \2/'
)"
if [[ -n $parsed_update_args ]]; then
  # shellcheck disable=SC2086
  _ scripts/cargo-for-all-lock-files.sh update $parsed_update_args
fi

echo --- ok
