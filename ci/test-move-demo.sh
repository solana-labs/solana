#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ci/test-move-demo.sh \
|| {
  annotate --style info --context test-bench \
    "Bench skipped as no .rs files were modified"
  exit 0
}


source ci/_
source ci/upload-ci-artifact.sh

eval "$(ci/channel-info.sh)"
source ci/rust-version.sh stable

set -o pipefail
export RUST_BACKTRACE=1

# Run Move tests
_ cargo +"$rust_stable" test --manifest-path=programs/move_loader_program/Cargo.toml ${V:+--verbose}
_ cargo +"$rust_stable" test --manifest-path=programs/move_loader_api/Cargo.toml ${V:+--verbose}
