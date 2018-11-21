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

_ cargo fmt -- --check
_ cargo clippy -- --version
_ cargo clippy -- --deny=warnings

_ ci/audit.sh
