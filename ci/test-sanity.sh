#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_

(
  echo --- git diff --check
  set -x

  if [[ -n $CI_BASE_BRANCH ]]
  then branch="$CI_BASE_BRANCH"
  else branch="master"
  fi

  # Look for failed mergify.io backports by searching leftover conflict markers
  # Also check for any trailing whitespaces!
  git fetch origin "$branch"
  git diff "$(git merge-base HEAD "origin/$branch")" --check --oneline
)

echo

_ ci/check-channel-version.sh
_ ci/nits.sh
_ ci/check-ssh-keys.sh

echo --- ok
