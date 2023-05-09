#!/usr/bin/env bash

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    # copy toolchain file to use solana's rust version
    cp "$SOLANA_DIR"/rust-toolchain.toml example-helloworld/
    cd example-helloworld || exit 1

    update_solana_dependencies src/program-rust "$SOLANA_VER"
    patch_crates_io_solana src/program-rust/Cargo.toml "$SOLANA_DIR"
    echo "[workspace]" >> src/program-rust/Cargo.toml

    $CARGO_BUILD_SBF \
      --manifest-path src/program-rust/Cargo.toml

    # TODO: Build src/program-c/...
  )
}
