#!/usr/bin/env bash
set -ex

SOLANA_ROOT="$(cd "$(dirname "$0")"/../../../../..; pwd)"
[[ -f "$SOLANA_ROOT"/LICENSE && -d "$SOLANA_ROOT"/ci ]]

rm -rf bpf-sdk/
mkdir bpf-sdk/

(
  "$SOLANA_ROOT"/ci/crate-version.sh
  git rev-parse HEAD
) > bpf-sdk/version.txt

"$SOLANA_ROOT"/programs/bpf/c/sdk/scripts/install.sh
cp -ra "$SOLANA_ROOT"/programs/bpf/c/sdk/* bpf-sdk/
rm -rf bpf-sdk/scripts/

tar jvcf bpf-sdk.tar.bz2 bpf-sdk/
