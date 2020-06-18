#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_

(
  echo --- git diff --check
  set -x
  # Look for failed mergify.io backports by searching leftover conflict markers
  # Also check for any trailing whitespaces!
  git fetch origin "$CI_BASE_BRANCH"
  git diff "$(git merge-base HEAD "origin/$CI_BASE_BRANCH")..HEAD" --check --oneline
)

echo

_ ci/nits.sh
_ ci/check-ssh-keys.sh

echo --- ok
