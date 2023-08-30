#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

export RUST_LOG="solana_metrics=warn,info,$RUST_LOG"

echo --- ci/localnet-sanity.sh
"$here"/../localnet-sanity.sh -x

echo --- ci/run-sanity.sh
"$here"/../run-sanity.sh -x
