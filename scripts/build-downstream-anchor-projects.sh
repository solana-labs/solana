#!/usr/bin/env bash
#
# Builds known downstream projects against local solana source
#

set -e
cd "$(dirname "$0")"/..
source ci/_
source scripts/patch-crates.sh
source scripts/read-cargo-variable.sh

anchor_version=$1
solana_ver=$(readCargoVariable version Cargo.toml)
solana_dir=$PWD
cargo="$solana_dir"/cargo
cargo_build_sbf="$solana_dir"/cargo-build-sbf
cargo_test_sbf="$solana_dir"/cargo-test-sbf

mkdir -p target/downstream-projects-anchor
cd target/downstream-projects-anchor

update_anchor_dependencies() {
  declare project_root="$1"
  declare anchor_ver="$2"
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$project_root" -name Cargo.toml)

  sed -i -e "s#\(anchor-lang = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-spl = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-lang = { version = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(anchor-spl = { version = \"\)[^\"]*\(\"\)#\1=$anchor_ver\2#g" "${tomls[@]}" || return $?
}

patch_crates_io_anchor() {
  declare Cargo_toml="$1"
  declare anchor_dir="$2"
  cat >> "$Cargo_toml" <<EOF
anchor-lang = { path = "$anchor_dir/lang" }
anchor-spl = { path = "$anchor_dir/spl" }
EOF
}

# NOTE This isn't run in a subshell to get $anchor_dir and $anchor_ver
anchor() {
  set -x
  rm -rf anchor
  git clone https://github.com/coral-xyz/anchor.git
  cd anchor || exit 1

  # checkout tag
  if [[ -n "$anchor_version" ]]; then
    git checkout "$anchor_version"
  fi

  # copy toolchain file to use solana's rust version
  cp "$solana_dir"/rust-toolchain.toml .

  update_solana_dependencies . "$solana_ver"
  patch_crates_io_solana Cargo.toml "$solana_dir"

  $cargo test
  (cd spl && $cargo_build_sbf --features dex metadata stake)
  (cd client && $cargo test --all-features)

  anchor_dir=$PWD
  anchor_ver=$(readCargoVariable version "$anchor_dir"/lang/Cargo.toml)

  cd "$solana_dir"/target/downstream-projects-anchor
}

mango() {
  (
    set -x
    rm -rf mango-v3
    git clone https://github.com/blockworks-foundation/mango-v3
    # copy toolchain file to use solana's rust version
    cp "$solana_dir"/rust-toolchain.toml mango-v3/
    cd mango-v3

    update_solana_dependencies . "$solana_ver"
    update_anchor_dependencies . "$anchor_ver"
    patch_crates_io_solana Cargo.toml "$solana_dir"
    patch_crates_io_anchor Cargo.toml "$anchor_dir"

    cd program
    $cargo build
    $cargo test
    $cargo_build_sbf
    $cargo_test_sbf
  )
}

metaplex() {
  (
    set -x
    rm -rf mpl-token-metadata
    git clone https://github.com/metaplex-foundation/mpl-token-metadata
    # copy toolchain file to use solana's rust version
    cp "$solana_dir"/rust-toolchain.toml mpl-token-metadata/
    cd mpl-token-metadata/programs/token-metadata/program

    update_solana_dependencies . "$solana_ver"
    patch_crates_io_solana Cargo.toml "$solana_dir"

    $cargo build
    $cargo test
    $cargo_build_sbf
    $cargo_test_sbf
  )
}

_ anchor
#_ metaplex
#_ mango
