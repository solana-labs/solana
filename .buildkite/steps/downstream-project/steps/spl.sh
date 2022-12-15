#!/usr/bin/env bash
set -e
cd "$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source .buildkite/steps/downstream-project/steps/common.sh

# Mind the order!
PROGRAMS=(
  instruction-padding/program
  token/program
  token/program-2022
  token/program-2022-test
  associated-token-account/program
  token-upgrade/program
  feature-proposal/program
  governance/addin-mock/program
  governance/program
  memo/program
  name-service/program
  stake-pool/program
)
set -x
rm -rf spl
git clone https://github.com/solana-labs/solana-program-library.git spl
cd spl

project_used_solana_version=$(sed -nE 's/solana-sdk = \"[>=<~]*(.*)\"/\1/p' <"token/program/Cargo.toml")
echo "used solana version: $project_used_solana_version"
if semverGT "$project_used_solana_version" "$SOLANA_VER"; then
  echo "skip"
  return
fi

./patch.crates-io.sh "$SOLANA_DIR"

for program in "${PROGRAMS[@]}"; do
  $CARGO_TEST_SBF --manifest-path "$program"/Cargo.toml
done

# TODO better: `build.rs` for spl-token-cli doesn't seem to properly build
# the required programs to run the tests, so instead we run the tests
# after we know programs have been built
$CARGO build
$CARGO test
