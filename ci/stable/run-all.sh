#!/usr/bin/env bash
set -eo pipefail

here="$(dirname "$0")"

#shellcheck source=ci/common/shared-functions.sh
source "$here"/../common/shared-functions.sh

#shellcheck source=ci/common/limit-threads.sh
source "$here"/../common/limit-threads.sh

#shellcheck source=ci/stable/common.sh
source "$here"/common.sh

_ ci/intercept.sh cargo test --jobs "$JOBS" --workspace --tests --verbose -- --nocapture
