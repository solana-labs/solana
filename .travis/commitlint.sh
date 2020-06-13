#!/usr/bin/env bash
#
# Runs commitlint in the provided subdirectory
#

set -e

basedir=$1
if [[ -z "$basedir" ]]; then
  basedir=.
fi

if [[ ! -d "$basedir" ]]; then
  echo "Error: not a directory: $basedir"
  exit 1
fi

if [[ ! -f "$basedir"/commitlint.config.js ]]; then
  echo "Error: No commitlint configuration found"
  exit 1
fi

if [[ -z $TRAVIS_COMMIT_RANGE ]]; then
  echo "Error: TRAVIS_COMMIT_RANGE not defined"
  exit 1
fi

cd "$basedir"
while IFS= read -r line; do
  echo "$line" | npx commitlint
done < <(git log "$TRAVIS_COMMIT_RANGE" --format=%s -- .)
