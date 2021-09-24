#!/usr/bin/env bash

set -e

here="$(dirname "$0")"
src_root="$(readlink -f "${here}/..")"

cd "${src_root}"

cargo_audit_ignores=(
  # failure is officially deprecated/unmaintained
  #
  # Blocked on multiple upstream crates removing their `failure` dependency.
  --ignore RUSTSEC-2020-0036

  # `net2` crate has been deprecated; use `socket2` instead
  #
  # Blocked on https://github.com/paritytech/jsonrpc/issues/575
  --ignore RUSTSEC-2020-0016

  # stdweb is unmaintained
  #
  # Blocked on multiple upstream crates removing their `stdweb` dependency.
  --ignore RUSTSEC-2020-0056

  # Potential segfault in the time crate
  #
  # Blocked on multiple crates updating `time` to >= 0.2.23
  --ignore RUSTSEC-2020-0071

  # generic-array: arr! macro erases lifetimes
  #
  # Blocked on libsecp256k1 releasing with upgraded dependencies
  # https://github.com/paritytech/libsecp256k1/issues/66
  --ignore RUSTSEC-2020-0146

  # hyper: Lenient `hyper` header parsing of `Content-Length` could allow request smuggling
  #
  # Blocked on jsonrpc removing dependency on unmaintained `websocket`
  # https://github.com/paritytech/jsonrpc/issues/605
  --ignore RUSTSEC-2021-0078

  # hyper: Integer overflow in `hyper`'s parsing of the `Transfer-Encoding` header leads to data loss
  #
  # Blocked on jsonrpc removing dependency on unmaintained `websocket`
  # https://github.com/paritytech/jsonrpc/issues/605
  --ignore RUSTSEC-2021-0079

  # zeroize_derive: `#[zeroize(drop)]` doesn't implement `Drop` for `enum`s
  --ignore RUSTSEC-2021-0115
)
scripts/cargo-for-all-lock-files.sh stable audit "${cargo_audit_ignores[@]}"
