#!/usr/bin/env bash
#
# Builds known downstream projects against local safecoin source
#

set -e
cd "$(dirname "$0")"/..
source ci/_
source scripts/read-cargo-variable.sh

safecoin_ver=$(readCargoVariable version sdk/Cargo.toml)
safecoin_dir=$PWD
cargo="$safecoin_dir"/cargo
cargo_build_bpf="$safecoin_dir"/cargo-build-bpf
cargo_test_bpf="$safecoin_dir"/cargo-test-bpf

mkdir -p target/downstream-projects
cd target/downstream-projects

update_safecoin_dependencies() {
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$1" -name Cargo.toml)

  sed -i -e "s#\(solana-program = \"\)[^\"]*\(\"\)#\1$safecoin_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(safecoin-sdk = \"\).*\(\"\)#\1$safecoin_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(safecoin-sdk = { version = \"\)[^\"]*\(\"\)#\1$safecoin_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(safecoin-client = \"\)[^\"]*\(\"\)#\1$safecoin_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(safecoin-client = { version = \"\)[^\"]*\(\"\)#\1$safecoin_ver\2#g" "${tomls[@]}" || return $?
}

patch_crates_io() {
  cat >> "$1" <<EOF
[patch.crates-io]
safecoin-client = { path = "$safecoin_dir/client" }
solana-program = { path = "$safecoin_dir/sdk/program" }
safecoin-sdk = { path = "$safecoin_dir/sdk" }
EOF
}

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    cd example-helloworld

    update_safecoin_dependencies src/program-rust
    patch_crates_io src/program-rust/Cargo.toml
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

    ./patch.crates-io.sh "$safecoin_dir"

    $cargo build

    # Generic `cargo test`/`cargo test-bpf` disabled due to BPF VM interface changes between Safecoin 1.4
    # and 1.5...
    #$cargo test
    #$cargo_test_bpf

    $cargo_test_bpf --manifest-path token/program/Cargo.toml
    $cargo_test_bpf --manifest-path associated-token-account/program/Cargo.toml
    $cargo_test_bpf --manifest-path feature-proposal/program/Cargo.toml
  )
}

serum_dex() {
  (
    set -x
    rm -rf serum-dex
    git clone https://github.com/project-serum/serum-dex.git
    cd serum-dex

    update_safecoin_dependencies .
    patch_crates_io Cargo.toml
    patch_crates_io dex/Cargo.toml
    cat >> dex/Cargo.toml <<EOF
[workspace]
exclude = [
    "crank",
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
