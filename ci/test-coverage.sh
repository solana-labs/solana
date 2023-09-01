#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

source ci/upload-ci-artifact.sh
source scripts/ulimit-n.sh

scripts/coverage.sh -p solana-sdk abi

if [[ -z $CI ]]; then
  exit
fi

report=coverage-"${CI_COMMIT:0:9}".tar.gz
mv target/cov/report.tar.gz "$report"
upload-ci-artifact "$report"

annotate --style success --context lcov-report \
  "lcov report: <a href=\"artifact://$report\">$report</a>"

echo "--- codecov.io report"
if [[ -z "$CODECOV_TOKEN" ]]; then
  echo "^^^ +++"
  echo CODECOV_TOKEN undefined, codecov.io upload skipped
else
  (
  set -x
  echo "$e==>$x Buildkite CI detected."
  urlencode() {
      echo "$1" | curl -Gso /dev/null -w "%{url_effective}" --data-urlencode @- "" | cut -c 3- | sed -e 's/%0A//'
  }
  service="buildkite"
  branch="$BUILDKITE_BRANCH"
  build="$BUILDKITE_BUILD_NUMBER"
  job="$BUILDKITE_JOB_ID"
  build_url=$(urlencode "$BUILDKITE_BUILD_URL")
  slug="$BUILDKITE_PROJECT_SLUG"
  commit="$BUILDKITE_COMMIT"
  if [[ "$BUILDKITE_PULL_REQUEST" != "false" ]]; then
    pr="$BUILDKITE_PULL_REQUEST"
  fi
  tag="$BUILDKITE_TAG"
  )
  # We normalize CI to `1`; but codecov expects it to be `true` to detect Buildkite...
  # Unfortunately, codecov.io fails sometimes:
  #   curl: (7) Failed to connect to codecov.io port 443: Connection timed out
  CI=true bash <(while ! curl -sS --retry 5 --retry-delay 2 --retry-connrefused --fail https://codecov.io/bash; do sleep 10; done) -Z -X gcov -f target/cov/lcov.info -b "$BUILDKITE_BRANCH"

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/solana-labs/solana/commit/${CI_COMMIT:0:9}"
fi
