#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"/../../..

rm -rf bpf-sdk.tar.bz2 bpf-sdk/
mkdir bpf-sdk/
cp LICENSE bpf-sdk/

(
  ci/crate-version.sh
  git rev-parse HEAD
) > bpf-sdk/version.txt

cp -ra sdk/bpf/* bpf-sdk/
rm -f bpf-sdk/scripts/package.sh

tar jvcf bpf-sdk.tar.bz2 bpf-sdk/
