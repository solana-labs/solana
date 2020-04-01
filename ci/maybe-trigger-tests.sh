#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

# Skip if only the docs have been modified
ci/affects-files.sh \
  \!^docs/ \
|| {
  annotate --style info \
    "Skipping all further tests as only docs/ files were modified"
  exit 0
}

annotate --style info "Triggering tests"
buildkite-agent pipeline upload ci/buildkite-tests.yml
