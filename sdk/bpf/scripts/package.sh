#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"/../../..

rm -rf bpf-sdk.tar.bz2 bpf-sdk/
mkdir bpf-sdk/
cp LICENSE bpf-sdk/

(
  ci/crate-version.sh sdk/Cargo.toml
  git rev-parse HEAD
) > bpf-sdk/version.txt

cp -a sdk/bpf/* bpf-sdk/

tar jvcf bpf-sdk.tar.bz2 bpf-sdk/
