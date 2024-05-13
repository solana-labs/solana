#!/usr/bin/env bash

set -e
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)

# shellcheck source=ci/upload-ci-artifact.sh
source "$here/upload-ci-artifact.sh"

if [[ -z $CI ]]; then
  echo "This script is used by CI environment. \
Use \`scripts/coverage.sh\` directly if you only want to obtain the coverage report"
  exit 1
fi

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

# run coverage for all
SHORT_CI_COMMIT=${CI_COMMIT:0:9}
COMMIT_HASH=$SHORT_CI_COMMIT "$here/../scripts/coverage.sh" "$@"

# compress coverage reports
HTML_REPORT_TAR_NAME="coverage.tar.gz"
HTML_REPORT_TAR_PATH="$here/../target/cov/${SHORT_CI_COMMIT}/$HTML_REPORT_TAR_NAME"
tar zcf "$HTML_REPORT_TAR_PATH" "$here/../target/cov/${SHORT_CI_COMMIT}/coverage"

# upload reports to buildkite
upload-ci-artifact "$HTML_REPORT_TAR_PATH"
annotate --style success --context lcov-report \
  "lcov report: <a href=\"artifact://$HTML_REPORT_TAR_NAME\">$HTML_REPORT_TAR_NAME</a>"

echo "--- codecov.io report"
if [[ -z "$CODECOV_TOKEN" ]]; then
  echo "^^^ +++"
  echo CODECOV_TOKEN undefined, codecov.io upload skipped
else
  codecov -t "${CODECOV_TOKEN}" --dir "$here/../target/cov/${SHORT_CI_COMMIT}"

  annotate --style success --context codecov.io \
    "CodeCov report: https://codecov.io/github/anza-xyz/agave/commit/$CI_COMMIT"
fi
