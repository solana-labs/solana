#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

(
    for project in programs/bpf/rust/*/ ; do
    (
        cd "$project"
        _ cargo +"$rust_stable" fmt --all -- --check
        _ cargo +"$rust_nightly" clippy --all -- --version
        _ cargo +"$rust_nightly" clippy --all -- --deny=warnings
        _ cargo +"$rust_stable" audit
    )
    done
)

_ cargo +"$rust_stable" fmt --all -- --check
_ cargo +"$rust_stable" clippy --all -- --version
_ cargo +"$rust_stable" clippy --all -- --deny=warnings
_ cargo +"$rust_stable" audit
_ ci/nits.sh
_ ci/order-crates-for-publishing.py
_ book/build.sh

echo --- ok
