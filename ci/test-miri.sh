#!/usr/bin/env bash

set -eo pipefail

source ci/rust-version.sh nightly

# miri is very slow; so only run very few of selective tests!
cargo "+${rust_nightly}" miri test -p solana-program -- hash:: account_info::
