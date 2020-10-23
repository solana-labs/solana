#!/usr/bin/env bash
#
# Builds known downstream projects against local solana source
#

set -e
cd "$(dirname "$0")"/..
source ci/_
source scripts/read-cargo-variable.sh

solana_ver=$(readCargoVariable version sdk/Cargo.toml)
solana_dir=$PWD

mkdir -p target/downstream-projects
cd target/downstream-projects

update_solana_dependencies() {
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$1" -name Cargo.toml)

  sed -i -e "s#\(solana-sdk = \"\).*\(\"\)#\1$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-sdk = { version = \"\).*\(\"\)#\1$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-client = \"\).*\(\"\)#\1$solana_ver\2#g" "${tomls[@]}" || return $?
}

update_spl_token_dependencies() {
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$1" -name Cargo.toml)

  declare spl_token_ver="$2"
  sed -i -e "s#\(spl-token = { version = \"\).*\(\"\)#\1$spl_token_ver\2#g" "${tomls[@]}" || return $?
}

patch_crates_io() {
  cat >> "$1" <<EOF
[patch.crates-io]
solana-client = { path = "$solana_dir/client"}
solana-sdk = { path = "$solana_dir/sdk" }
EOF
}

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    cd example-helloworld

    update_solana_dependencies src/program-rust
    patch_crates_io src/program-rust/Cargo.toml
    echo "[workspace]" >> src/program-rust/Cargo.toml

    "$solana_dir"/cargo-build-bpf \
      --manifest-path src/program-rust/Cargo.toml \
      --no-default-features --features program

    # TODO: Build src/program-c/...
  )
}

spl() {
  (
    set -x
    rm -rf spl
    git clone https://github.com/solana-labs/solana-program-library.git spl
    cd spl

    update_solana_dependencies .
    patch_crates_io Cargo.toml

    "$solana_dir"/cargo-build-bpf \
      --manifest-path memo/program/Cargo.toml \
      --no-default-features --features program

    "$solana_dir"/cargo-build-bpf \
      --manifest-path token/program/Cargo.toml \
      --no-default-features --features program
  )
}

serum_dex() {
  (
    set -x
    rm -rf serum-dex
    git clone https://github.com/project-serum/serum-dex.git  # TODO: Consider using a tag
    cd serum-dex

    update_solana_dependencies .
    update_spl_token_dependencies . 2.0.8
    patch_crates_io Cargo.toml
    patch_crates_io dex/Cargo.toml
    echo "[workspace]" >> dex/Cargo.toml

    "$solana_dir"/cargo stable build

    "$solana_dir"/cargo-build-bpf \
      --manifest-path dex/Cargo.toml --no-default-features --features program

    "$solana_dir"/cargo stable test \
      --manifest-path dex/Cargo.toml --no-default-features --features program
  )
}


_ example_helloworld
_ spl
_ serum_dex
