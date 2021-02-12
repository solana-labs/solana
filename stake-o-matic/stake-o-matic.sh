#!/usr/bin/env bash
#
# Downloads and runs the latest stake-o-matic binary
#
set -e

solana_version=edge
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh \
    | sh -s - $solana_version \
        --no-modify-path \
        --data-dir ./solana-install \
        --config ./solana-install/config.yml

PATH="$(realpath "$PWD"/solana-install/releases/"$solana_version"*/solana-release/bin/):$PATH"
echo PATH="$PATH"

set -x
solana --version
exec solana-stake-o-matic "$@"
