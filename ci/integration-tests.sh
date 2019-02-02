#!/usr/bin/env bash
#
# Runs all integration tests in the tree serially
#
set -e
FEATURES="$1"
cd "$(dirname "$0")/.."
source ci/_
export RUST_BACKTRACE=1

for test in {,*/}tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  (
    export RUST_LOG="$test"=trace,$RUST_LOG
    _ cargo test --all --verbose --features="$FEATURES" --test="$test" \
      -- --test-threads=1 --nocapture
  )
done
