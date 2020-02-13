#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ^ci/rust-version.sh \
  ^ci/test-coverage.sh \
  ^scripts/coverage.sh \
|| {
  annotate --style info --context test-coverage \
    "Coverage skipped as no .rs files were modified"
  exit 0
}

source ci/upload-ci-artifact.sh
source scripts/ulimit-n.sh

scripts/coverage.sh

report=coverage-"${CI_COMMIT:0:9}".tar.gz
mv target/cov/report.tar.gz "$report"
upload-ci-artifact "$report"

gzip -f target/cov/coverage-stderr.log
upload-ci-artifact target/cov/coverage-stderr.log.gz

annotate --style success --context lcov-report \
  "lcov report: <a href=\"artifact://$report\">$report</a>"

echo "--- codecov.io report"
if [[ -z "$CODECOV_TOKEN" ]]; then
  echo "^^^ +++"
  echo CODECOV_TOKEN undefined, codecov.io upload skipped
else
  # We normalize CI to `1`; but codecov expects it to be `true` to detect Buildkite...
  # Unfortunately, codecov.io fails sometimes:
  #   curl: (7) Failed to connect to codecov.io port 443: Connection timed out
  CI=true bash <(while ! curl -sS --retry 5 --retry-delay 2 --retry-connrefused https://codecov.io/bash; do sleep 10; done) -Z -X gcov -f target/cov/lcov.info

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/solana-labs/solana/commit/${CI_COMMIT:0:9}"
fi
