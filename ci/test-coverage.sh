#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

if ci/is-pr.sh; then
  affectedFiles="$(buildkite-agent meta-data get affected_files)"
  echo "Affected files in this PR: $affectedFiles"
  if [[ ! ":$affectedFiles:" =~ \.rs: ]]; then
    annotate --style info --context coverage-info \
      "Coverage skipped as no .rs files were modified"
    exit 0
  fi
fi

source ci/upload-ci-artifact.sh
ci/version-check-with-upgrade.sh nightly

scripts/coverage.sh

report=coverage-"${BUILDKITE_COMMIT:0:9}".tar.gz
mv target/cov/report.tar.gz "$report"
upload-ci-artifact "$report"
annotate --style success --context lcov-report \
  "lcov report: <a href=\"artifact://$report\">$report</a>"

echo "--- codecov.io report"
if [[ -z "$CODECOV_TOKEN" ]]; then
  echo "^^^ +++"
  echo CODECOV_TOKEN undefined, codecov.io upload skipped
else
  bash <(curl -s https://codecov.io/bash) -X gcov -f target/cov/lcov.info

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/solana-labs/solana/commit/${BUILDKITE_COMMIT:0:9}"
fi
