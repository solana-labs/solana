#!/usr/bin/env bash
#
# Runs all integration tests in the tree serially
#
set -e
maybeFeatures=
if [[ -n $1 ]]; then
  maybeFeatures="--features=$1"
fi

cd "$(dirname "$0")/.."
source ci/_
export RUST_BACKTRACE=1
source scripts/ulimit-n.sh

for test in {,*/}tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  (
    export RUST_LOG="$test"=trace,$RUST_LOG
    # shellcheck disable=SC2086 # Don't want to double quote $maybeFeatures
    _ cargo test --all --verbose $maybeFeatures --test="$test" \
      -- --test-threads=1 --nocapture
  )
done
