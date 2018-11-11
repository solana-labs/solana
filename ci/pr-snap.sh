#!/usr/bin/env bash
set -e
#
# Only run snap.sh for pull requests that modify files under /snap
#

cd "$(dirname "$0")"

if ./is-pr.sh; then
  affected_files="$(buildkite-agent meta-data get affected_files)"
  echo "Affected files in this PR: $affected_files"
  if [[ ! ":$affected_files:" =~ :snap/ ]]; then
    echo "Skipping snap build as no files under /snap were modified"
    exit 0
  fi
  exec ./snap.sh
else
  echo "Skipping snap build as this is not a pull request"
fi
