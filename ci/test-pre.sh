#!/usr/bin/env bash

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ci/test-stable-perf.sh \
  ci/test-stable.sh \
|| {
  annotate --style info \
    "Skipped tests as no relavant files were modified"
  exit 0
}

ci/version-check-with-upgrade.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"
