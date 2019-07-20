#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

do_bpf_check() {
        _ cargo +"$rust_stable" fmt --all -- --check
        _ cargo +"$rust_nightly" test --all
        _ cargo +"$rust_nightly" clippy --all -- --version
        _ cargo +"$rust_nightly" clippy --all -- --deny=warnings
#        _ cargo +"$rust_stable" audit
}

(
    (
        cd sdk/bpf/rust/rust-no-std
        do_bpf_check
    )
    (
        cd sdk/bpf/rust/rust-utils
        do_bpf_check
    )
    (
        cd sdk/bpf/rust/rust-test
        do_bpf_check
    )
    for project in programs/bpf/rust/*/ ; do
    (
        cd "$project"
        do_bpf_check
    )
    done
)

_ cargo +"$rust_stable" fmt --all -- --check
_ cargo +"$rust_stable" clippy --all -- --version
_ cargo +"$rust_stable" clippy --all -- --deny=warnings
#_ cargo +"$rust_stable" audit
_ ci/nits.sh
_ ci/order-crates-for-publishing.py
_ book/build.sh

echo --- ok
