#!/usr/bin/env bash
set -e
cd "$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source .buildkite/steps/downstream-project/steps/common.sh

set -x
rm -rf example-helloworld
git clone https://github.com/solana-labs/example-helloworld.git
cd example-helloworld

update_solana_dependencies src/program-rust "$SOLANA_VER"
patch_crates_io_solana src/program-rust/Cargo.toml "$SOLANA_DIR"
echo "[workspace]" >>src/program-rust/Cargo.toml

$CARGO_BUILD_SBF \
  --manifest-path src/program-rust/Cargo.toml
