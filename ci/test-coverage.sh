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
  # We normalize CI to `1`; but codecov expects it to be `true` to detect Buildkite...
  # Unfortunately, codecov.io fails sometimes:
  #   curl: (7) Failed to connect to codecov.io port 443: Connection timed out
  retry=5
  while :; do
    if ((retry > 0)); then
      echo "fetching coverage_bash_script..."
    else
      echo "can't fetch coverage_bash_script successfully"
      exit 1
    fi

    http_code=$(curl -s -o coverage_bash_script -w "%{http_code}" https://codecov.io/bash)
    if [[ "$http_code" = "200" ]]; then
      break
    fi

    echo "got http_code $http_code"
    retry="$((retry - 1))"
    sleep 2
  done
  CI=true bash <coverage_bash_script -Z -X gcov -f target/cov/lcov.info

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/solana-labs/solana/commit/${CI_COMMIT:0:9}"
fi
