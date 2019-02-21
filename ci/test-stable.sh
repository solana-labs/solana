#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/test-pre.sh

_ cargo build --all ${V:+--verbose}
_ cargo test --all ${V:+--verbose} -- --nocapture --test-threads=1

exec ci/test-post.sh
