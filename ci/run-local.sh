#!/usr/bin/env bash -e
#
# Run the entire buildkite CI pipeline locally for pre-testing before sending a
# Github pull request
#

cd "$(dirname "$0")/.."
BKRUN=ci/node_modules/.bin/bkrun

if [[ ! -x $BKRUN ]]; then
  (
    set -x
    cd ci/
    npm install bkrun
  )
fi

set -x
exec ./ci/node_modules/.bin/bkrun ci/buildkite.yml
