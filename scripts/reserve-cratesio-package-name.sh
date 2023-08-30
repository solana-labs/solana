#!/usr/bin/env bash

display_help() {
  local bin
  bin="$(basename --suffix='.sh' "$0")"
  cat <<EOF 1>&2
$bin
Reserve a Rust package name on crates.io

USAGE:
    $bin [FLAGS] [OPTIONS] <TARGET_TYPE> <PACKAGE_NAME>

FLAGS:
        --help                Display this help message
        --no-solana-prefix    Do not require \`solana-\` prefix on PACKAGE_NAME
        --publish             Upload the reserved package. Without this flag, a
                              dry-run is performed

OPTIONS:
        --token <TOKEN>       Token used to authenticate with crates.io

ARGS:
    TARGET_TYPE     The package target type [possible values: bin, lib]
    PACKAGE_NAME    The desired package name [see:
                    https://doc.rust-lang.org/cargo/reference/manifest.html#the-name-field]
EOF
}

require_solana_prefix=true
maybe_publish='--dry-run'
positional=()
while [[ -n "$1" ]]; do
  case "$1" in
    --)
      break
      ;;
    --help)
      display_help
      exit 0
      ;;
    --no-solana-prefix)
      require_solana_prefix=false
      ;;
    --publish)
      maybe_publish=''
      ;;
    --token)
      maybe_crates_token="--token $2"
      shift
      ;;
    --* | -*)
      echo "error: unexpected argument \`$1\`" 1>&2
      display_help
      exit 1
      ;;
    *)
      positional+=("$1")
      ;;
  esac
  shift
done
while [[ -n "$1" ]]; do
  positional+=("$1")
  shift
done

target_type="${positional[0]:?'TARGET_TYPE must be declared'}"
package_name="${positional[1]:?'PACKAGE_NAME must be declared'}"

case "${target_type}" in
  bin)
    src_filename='main.rs'
    src_file_body='fn main() {}'
    ;;
  lib)
    src_filename='lib.rs'
    src_file_body=''
    ;;
  *)
    echo "error: unexpected TARGET_TYPE: \`${target_type}\`" 1>&2
    display_help
    exit 1
    ;;
esac

if ! [[ "${package_name}" =~ ^[a-zA-Z0-9_-]{1,64} ]]; then
  echo "error: illegal PACKAGE_NAME: \`${package_name}\`" 1>&2
  display_help
  exit 1
fi

if ${require_solana_prefix} && ! [[ "${package_name}" =~ ^solana- ]]; then
  # shellcheck disable=SC2016 # backticks are not a command here
  echo 'error: PACKAGE_NAME MUST start with `solana-`' 1>&2
  display_help
  exit 1
fi

tmpdir="$(mktemp -d)"
if pushd "${tmpdir}" &>/dev/null; then
  cat <<EOF > "Cargo.toml"
[package]
name = "${package_name}"
version = "0.0.0"
description = "reserved for future use"
authors = ["Solana Labs Maintainers <maintainers@solanalabs.com>"]
repository = "https://github.com/solana-labs/solana"
license = "Apache-2.0"
homepage = "https://solanalabs.com"
documentation = "https://docs.rs/${package_name}"
edition = "2021"
EOF
  mkdir -p src
  echo "${src_file_body}" > "src/${src_filename}"
  # shellcheck disable=SC2086 # do not want to quote optional arg tokens
  cargo publish ${maybe_publish} ${maybe_crates_token}
  popd &>/dev/null || true
fi

rm -rf "${tmpdir}"
