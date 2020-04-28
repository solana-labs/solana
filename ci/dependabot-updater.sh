#!/usr/bin/env bash

set -ex
cd "$(dirname "$0")/.."
source ci/_

commit_range="$(git merge-base HEAD origin/master)..HEAD"
parsed_update_args="$(
  git log "$commit_range" --author "dependabot-preview" --oneline -n1 |
    grep -o 'Bump.*$' |
    sed -r 's/Bump ([^ ]+) from ([^ ]+) to ([^ ]+)/-p \1:\2 --precise \3/'
)"
# relaxed_parsed_update_args is temporal measure...
relaxed_parsed_update_args="$(
  git log "$commit_range" --author "dependabot-preview" --oneline -n1 |
    grep -o 'Bump.*$' |
    sed -r 's/Bump ([^ ]+) from [^ ]+ to ([^ ]+)/-p \1 --precise \2/'
)"
package=$(echo "$parsed_update_args" | awk '{print $2}' | grep -o "^[^:]*")
if [[ -n $parsed_update_args ]]; then
  # find other Cargo.lock files and update them, excluding the default Cargo.lock
  # shellcheck disable=SC2086
  for lock in $(git grep --files-with-matches '^name = "'$package'"$' :**/Cargo.lock); do
    # it's possible our current versions are out of sync across lock files,
    # in that case try to sync them up with $relaxed_parsed_update_args
    _ scripts/cargo-for-all-lock-files.sh \
      "$lock" -- \
      update $parsed_update_args ||
      _ scripts/cargo-for-all-lock-files.sh \
        "$lock" -- \
        update $relaxed_parsed_update_args
  done
fi

echo --- ok
