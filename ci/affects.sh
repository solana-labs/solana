#!/usr/bin/env bash
#
# Check if files in the commit range match one or more prefixes
#

# Always run the job if we are on a tagged release
if [[ -n "$TRAVIS_TAG" ]]; then
  exit 0
fi

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
