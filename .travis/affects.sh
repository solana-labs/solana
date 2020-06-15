#!/usr/bin/env bash
#
# Check if files in the commit range match one or more prefixes
#

(
  set -x
  git diff --name-only "$TRAVIS_COMMIT_RANGE"
)

for file in $(git diff --name-only "$TRAVIS_COMMIT_RANGE"); do
  for prefix in "$@"; do
    if [[ $file =~ ^"$prefix" ]]; then
      exit 0
    fi
    done
done

echo "No modifications to $*"
exit 1
