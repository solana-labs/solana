#!/usr/bin/env bash
#
# Builds a buildkite pipeline based on the environment variables
#

set -e
cd "$(dirname "$0")"/../..

output_file=${1:-/dev/stderr}

start_pipeline() {
  echo "# $*" > "$output_file"
  echo "steps:" >> "$output_file"
}

wait_step() {
  echo "  - wait" >> "$output_file"
}

cat_steps() {
  cat "$@" >> "$output_file"
}

pull_or_push_steps() {
  cat_steps ci/buildkite/sanity.yml
  wait_step

  if ci/affects-files.sh .sh$; then
    cat_steps ci/buildkite/shellcheck.yml
  fi
  wait_step

  docs=false
  all_tests=false

  if ci/affects-files.sh ^docs/; then
    docs=true
  fi

  if ci/affects-files.sh !^docs/ !.md$ !^.buildkite/; then
    all_tests=true
  fi

  if $docs; then
    cat_steps ci/buildkite/docs.yml
  fi

  if $all_tests; then
    cat_steps ci/buildkite/all-tests.yml
  fi
}


if [[ -n $BUILDKITE_TAG ]]; then
  start_pipeline "Tag pipeline for $BUILDKITE_TAG"

  buildkite-agent annotate --style info --context release-tag \
    "https://github.com/solana-labs/solana/releases/$BUILDKITE_TAG"

  # Jump directly to the secondary build to publish release artifacts quickly
  cat_steps ci/buildkite/trigger-secondary.yml
  exit 0
fi


if [[ $BUILDKITE_BRANCH =~ ^pull ]]; then
  start_pipeline "Pull request pipeline for $BUILDKITE_BRANCH"

  # Add helpful link back to the corresponding Github Pull Request
  buildkite-agent annotate --style info --context pr-backlink \
    "Github Pull Request: https://github.com/solana-labs/solana/$BUILDKITE_BRANCH"

  cat_steps ci/buildkite/dependabot-pr.yml
  pull_or_push_steps
  exit 0
fi

start_pipeline "Push pipeline for $BUILDKITE_BRANCH"
pull_or_push_steps
wait_step
cat_steps ci/buildkite/trigger-secondary.yml
exit 0
