#!/usr/bin/env bash

set -eo pipefail

source ci/_
source ci/rust-version.sh nightly

# miri is very slow; so only run very few of selective tests!
_ cargo "+${rust_nightly}" miri test -p solana-program -- hash:: account_info::

_ cargo "+${rust_nightly}" miri test -p solana-unified-scheduler-logic

# run intentionally-#[ignored] ub triggering tests for each to make sure they fail
(! _ cargo "+${rust_nightly}" miri test -p solana-unified-scheduler-logic -- \
  --ignored --exact "utils::tests::test_ub_illegally_created_multiple_tokens")
(! _ cargo "+${rust_nightly}" miri test -p solana-unified-scheduler-logic -- \
  --ignored --exact "utils::tests::test_ub_illegally_shared_token_cell")
