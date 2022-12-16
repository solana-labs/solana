#!/usr/bin/env bash
#
# Builds known downstream projects against local solana source
#

set -e
cd "$(dirname "$0")"/..
source ci/_
source ci/semver_bash/semver.sh
source scripts/patch-crates.sh
source scripts/read-cargo-variable.sh

solana_ver=$(readCargoVariable version sdk/Cargo.toml)
solana_dir=$PWD
cargo_build_sbf="$solana_dir"/cargo-build-sbf
cargo_test_sbf="$solana_dir"/cargo-test-sbf

mkdir -p target/downstream-projects
cd target/downstream-projects

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    # copy toolchain file to use solana's rust version
    cp "$solana_dir"/rust-toolchain.toml example-helloworld/
    cd example-helloworld

    update_solana_dependencies src/program-rust "$solana_ver"
    patch_crates_io_solana src/program-rust/Cargo.toml "$solana_dir"
    echo "[workspace]" >> src/program-rust/Cargo.toml

    $cargo_build_sbf \
      --manifest-path src/program-rust/Cargo.toml

    # TODO: Build src/program-c/...
  )
}

spl() {
  (
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
    # copy toolchain file to use solana's rust version
    cp "$solana_dir"/rust-toolchain.toml spl/
    cd spl

    project_used_solana_version=$(sed -nE 's/solana-sdk = \"[>=<~]*(.*)\"/\1/p' <"token/program/Cargo.toml")
    echo "used solana version: $project_used_solana_version"
    if semverGT "$project_used_solana_version" "$solana_ver"; then
      echo "skip"
      return
    fi

    ./patch.crates-io.sh "$solana_dir"

    for program in "${PROGRAMS[@]}"; do
      $cargo_test_sbf --manifest-path "$program"/Cargo.toml
    done

    # TODO better: `build.rs` for spl-token-cli doesn't seem to properly build
    # the required programs to run the tests, so instead we run the tests
    # after we know programs have been built
    cargo build
    cargo test
  )
}

openbook_dex() {
  (
    set -x
    rm -rf openbook-dex
    git clone https://github.com/openbook-dex/program.git openbook-dex
    # copy toolchain file to use solana's rust version
    cp "$solana_dir"/rust-toolchain.toml openbook-dex/
    cd openbook-dex

    update_solana_dependencies . "$solana_ver"
    patch_crates_io_solana Cargo.toml "$solana_dir"
    cat >> Cargo.toml <<EOF
anchor-lang = { git = "https://github.com/coral-xyz/anchor.git", branch = "master" }
EOF
    patch_crates_io_solana dex/Cargo.toml "$solana_dir"
    cat >> dex/Cargo.toml <<EOF
anchor-lang = { git = "https://github.com/coral-xyz/anchor.git", branch = "master" }
[workspace]
exclude = [
    "crank",
    "permissioned",
]
EOF
    cargo build

    $cargo_build_sbf \
      --manifest-path dex/Cargo.toml --no-default-features --features program

    cargo test \
      --manifest-path dex/Cargo.toml --no-default-features --features program
  )
}

_ example_helloworld
_ spl
_ openbook_dex
