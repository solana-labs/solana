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

_ ci/nits.sh
_ ci/check-ssh-keys.sh


# Ensure the current channel version is not equal ("greater") than
# the version of the latest tag
if [[ -z $CI_TAG ]]; then
  echo "--- channel version check"
  (
    eval "$(ci/channel-info.sh)"

    if [[ -n $CHANNEL_LATEST_TAG ]]; then
      source scripts/read-cargo-variable.sh

      version=$(readCargoVariable version "version/Cargo.toml")
      echo "latest channel tag: $CHANNEL_LATEST_TAG"
      echo "current version: v$version"

      if [[ $CHANNEL_LATEST_TAG = v$version ]]; then
        echo "Error: A release has been tagged since your feature branch was created. <current version> should be greater than <latest channel tag>.
        Possible solutions (in the order they should be tried):
        1. rebase your feature branch on the base branch
        2. merge the PR: \"Bump Version to <$version+1>\" and then rebase
        3. ask for help in #devops."
        exit 1
      fi
    else
      echo "Skipped. CHANNEL_LATEST_TAG (CHANNEL=$CHANNEL) unset"
    fi
  )
fi

echo --- ok
