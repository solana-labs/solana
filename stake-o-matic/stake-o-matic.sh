#!/usr/bin/env bash
#
# Downloads and runs the latest stake-o-matic binary
#
set -e

solana_version=edge
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/safecoin-install-init.sh \
    | sh -s - $solana_version \
        --no-modify-path \
        --data-dir ./safecoin-install \
        --config ./safecoin-install/config.yml

PATH="$(realpath "$PWD"/safecoin-install/releases/"$solana_version"*/solana-release/bin/):$PATH"
echo PATH="$PATH"

set -x
safecoin --version
exec safecoin-stake-o-matic "$@"
