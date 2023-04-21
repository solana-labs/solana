#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

#shellcheck source=ci/common/shared-functions.sh
source "$here"/../common/shared-functions.sh

#shellcheck source=ci/common/limit-threads.sh
source "$here"/../common/limit-threads.sh

#shellcheck source=ci/stable/common.sh
source "$here"/common.sh

if need_to_generate_test_result; then
  _ cargo test --jobs "$JOBS" --workspace --tests --verbose -- -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
else
  _ ci/intercept.sh cargo test --jobs "$JOBS" --workspace --tests --verbose -- --nocapture
fi
