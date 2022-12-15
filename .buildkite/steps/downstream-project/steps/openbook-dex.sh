#!/usr/bin/env bash
set -e
cd "$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source .buildkite/steps/downstream-project/steps/common.sh

set -x
rm -rf openbook-dex
git clone https://github.com/openbook-dex/program.git openbook-dex
cd openbook-dex

update_solana_dependencies . "$SOLANA_VER"
patch_crates_io_solana Cargo.toml "$SOLANA_DIR"
cat >>Cargo.toml <<EOF
anchor-lang = { git = "https://github.com/coral-xyz/anchor.git", branch = "master" }
EOF
patch_crates_io_solana dex/Cargo.toml "$SOLANA_DIR"
cat >>dex/Cargo.toml <<EOF
anchor-lang = { git = "https://github.com/coral-xyz/anchor.git", branch = "master" }
[workspace]
exclude = [
    "crank",
    "permissioned",
]
EOF
$CARGO build

$CARGO_BUILD_SBF \
  --manifest-path dex/Cargo.toml --no-default-features --features program

$CARGO test \
  --manifest-path dex/Cargo.toml --no-default-features --features program
