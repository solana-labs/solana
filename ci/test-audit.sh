#!/usr/bin/env bash
set -e

source ci/_
source ci/rust-version.sh audit

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_ cargo +"$rust_audit" audit --version
_ scripts/cargo-for-all-lock-files.sh +"$rust_audit" audit --ignore RUSTSEC-2020-0002 --ignore RUSTSEC-2020-0008
