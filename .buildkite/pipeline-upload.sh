#!/usr/bin/env bash
#
# This script is used to upload the full buildkite pipeline. The steps defined
# in the buildkite UI should simply be:
#
#   steps:
#    - command: ".buildkite/pipeline-upload.sh"
#

set -e
cd "$(dirname "$0")"/..

if [[ -n $BUILDKITE_TAG ]]; then
  buildkite-agent annotate --style info --context release-tag \
    "https://github.com/solana-labs/solana/releases/$BUILDKITE_TAG"
  buildkite-agent pipeline upload ci/buildkite-release.yml
else
  if [[ $BUILDKITE_BRANCH =~ ^pull ]]; then
    # Add helpful link back to the corresponding Github Pull Request
    buildkite-agent annotate --style info --context pr-backlink \
      "Github Pull Request: https://github.com/solana-labs/solana/$BUILDKITE_BRANCH"
  fi

  if [[ $BUILDKITE_MESSAGE =~ GitBook: ]]; then
    buildkite-agent annotate --style info --context gitbook-ci-skip \
      "GitBook commit detected, CI skipped"
    exit
  fi

  buildkite-agent pipeline upload ci/buildkite.yml
fi
