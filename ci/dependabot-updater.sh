#!/usr/bin/env bash

set -ex
cd "$(dirname "$0")/.."
source ci/_

commit_range="$(git merge-base HEAD origin/master)..HEAD"
parsed_update_args="$(
  git log "$commit_range" --author "dependabot-preview" --oneline -n1 |
    grep -o 'Bump.*$' |
    sed -r 's/Bump ([^ ]+) from [^ ]+ to ([^ ]+)/-p \1 --precise \2/'
)"
package=$(echo "$parsed_update_args" | awk '{print $2}')
if [[ -n $parsed_update_args ]]; then
  # shellcheck disable=SC2086
  _ scripts/cargo-for-all-lock-files.sh \
    "$(git grep --files-with-matches "$package" :**/Cargo.lock)" -- \
    update $parsed_update_args
fi

echo --- ok
