#!/usr/bin/env bash
set -e

source ci/_
source ci/semver_bash/semver.sh
source scripts/patch-crates.sh
source scripts/read-cargo-variable.sh

SOLANA_VER=$(readCargoVariable version Cargo.toml)
export SOLANA_VER
export SOLANA_DIR=$PWD
export CARGO="$SOLANA_DIR"/cargo
export CARGO_BUILD_SBF="$SOLANA_DIR"/cargo-build-sbf
export CARGO_TEST_SBF="$SOLANA_DIR"/cargo-test-sbf

mkdir -p target/downstream-projects
cd target/downstream-projects
