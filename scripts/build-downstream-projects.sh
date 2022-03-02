#!/usr/bin/env bash
#
# Builds known downstream projects against local solana source
#

set -e
cd "$(dirname "$0")"/..
source ci/_
source scripts/patch-crates.sh
source scripts/read-cargo-variable.sh

solana_ver=$(readCargoVariable version sdk/Cargo.toml)
solana_dir=$PWD
cargo="$solana_dir"/cargo
cargo_build_bpf="$solana_dir"/cargo-build-bpf
cargo_test_bpf="$solana_dir"/cargo-test-bpf

mkdir -p target/downstream-projects
cd target/downstream-projects

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    cd example-helloworld

    update_solana_dependencies src/program-rust "$solana_ver"
    patch_crates_io_solana src/program-rust/Cargo.toml "$solana_dir"
    echo "[workspace]" >> src/program-rust/Cargo.toml

    $cargo_build_bpf \
      --manifest-path src/program-rust/Cargo.toml

    # TODO: Build src/program-c/...
  )
}

spl() {
  (
    set -x
    rm -rf spl
    git clone https://github.com/solana-labs/solana-program-library.git spl
    cd spl

    ./patch.crates-io.sh "$solana_dir"

    $cargo build
    $cargo test
    $cargo_build_bpf
    $cargo_test_bpf
  )
}

serum_dex() {
  (
    set -x
    rm -rf serum-dex
    git clone https://github.com/project-serum/serum-dex.git
    cd serum-dex

    update_solana_dependencies . "$solana_ver"
    patch_crates_io_solana Cargo.toml "$solana_dir"
    patch_crates_io_solana dex/Cargo.toml "$solana_dir"
    cat >> dex/Cargo.toml <<EOF
[workspace]
exclude = [
    "crank",
    "permissioned",
]
EOF
    $cargo build

    $cargo_build_bpf \
      --manifest-path dex/Cargo.toml --no-default-features --features program

    $cargo test \
      --manifest-path dex/Cargo.toml --no-default-features --features program
  )
}

_ example_helloworld
_ spl
_ serum_dex
