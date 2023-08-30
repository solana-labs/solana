#!/usr/bin/env bash
set -e

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

source ci/_
