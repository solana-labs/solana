#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

# bench_streamer is disabled by default to speed up dev builds, enable it
# explicitly here to ensure it still passes clippy
FEATURES=bench_streamer

_ cargo fmt -- --check
_ cargo clippy -- --version
_ cargo clippy --features="$FEATURES" -- --deny=warnings

_ ci/audit.sh
