#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

_ cargo fmt -- --check
_ cargo build --verbose
_ cargo test --verbose

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)

_ ci/audit.sh || true

# Store binary tarball as a buildkite artifact
_ cargo install --path . --root farf
(
  cd farf
  tar zcvf ../solana-bin.tar.gz bin
)
ls -l solana-bin.tar.gz
source ci/upload_ci_artifact.sh
upload_ci_artifact solana-bin.tar.gz
