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

scripts/coverage.sh "$@"

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
  codecov -t "${CODECOV_TOKEN}"

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/solana-labs/solana/commit/${CI_COMMIT:0:9}"
fi
