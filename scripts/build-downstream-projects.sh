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
cargo="$solana_dir"/cargo
cargo_build_bpf="$solana_dir"/cargo-build-bpf
cargo_test_bpf="$solana_dir"/cargo-test-bpf

mkdir -p target/downstream-projects
cd target/downstream-projects

update_solana_dependencies() {
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$1" -name Cargo.toml)

  sed -i -e "s#\(solana-program = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-program-test = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-sdk = \"\).*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-sdk = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-client = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-client = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-clap-utils = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-clap-utils = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
}

update_anchor_dependencies() {
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$1" -name Cargo.toml)

  sed -i -e "s#\(anchor-lang = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-spl = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-lang = { version = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-spl = { version = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
}

patch_crates_io_solana() {
  cat >> "$1" <<EOF
[patch.crates-io]
solana-clap-utils = { path = "$solana_dir/clap-utils" }
solana-client = { path = "$solana_dir/client" }
solana-program = { path = "$solana_dir/sdk/program" }
solana-program-test = { path = "$solana_dir/program-test" }
solana-sdk = { path = "$solana_dir/sdk" }
EOF
}

patch_crates_io_anchor() {
  cat >> "$1" <<EOF
anchor-lang = { path = "$anchor_dir/lang" }
anchor-spl = { path = "$anchor_dir/spl" }
EOF
}

example_helloworld() {
  (
    set -x
    rm -rf example-helloworld
    git clone https://github.com/solana-labs/example-helloworld.git
    cd example-helloworld

    update_solana_dependencies src/program-rust
    patch_crates_io_solana src/program-rust/Cargo.toml
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

    update_solana_dependencies .
    patch_crates_io_solana Cargo.toml
    patch_crates_io_solana dex/Cargo.toml
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

# NOTE This isn't run in a subshell to get $anchor_dir and $anchor_ver
anchor() {
  set -x
  rm -rf anchor
  git clone https://github.com/project-serum/anchor.git
  cd anchor

  update_solana_dependencies .
  patch_crates_io_solana Cargo.toml

  $cargo build
  $cargo test

  anchor_dir=$PWD
  anchor_ver=$(readCargoVariable version "$anchor_dir"/lang/Cargo.toml)

  cd "$solana_dir"/target/downstream-projects
}

mango() {
  (
    set -x
    rm -rf mango-v3
    git clone https://github.com/blockworks-foundation/mango-v3
    cd mango-v3

    update_solana_dependencies .
    update_anchor_dependencies .
    patch_crates_io_solana Cargo.toml
    patch_crates_io_anchor Cargo.toml

    $cargo build
    $cargo test
    $cargo_build_bpf
    $cargo_test_bpf
  )
}

metaplex() {
  (
    set -x
    rm -rf metaplex
    git clone https://github.com/metaplex-foundation/metaplex
    cd metaplex/rust

    update_solana_dependencies .
    update_anchor_dependencies .
    patch_crates_io_solana Cargo.toml
    patch_crates_io_anchor Cargo.toml

    $cargo build
    $cargo test
    $cargo_build_bpf
    $cargo_test_bpf
  )
}

_ anchor
_ example_helloworld
_ spl
_ serum_dex
#_ metaplex
_ mango
