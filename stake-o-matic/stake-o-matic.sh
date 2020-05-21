#!/usr/bin/env bash
#
# Downloads and runs the latest stake-o-matic binary
#

solana_version=edge
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh \
    | sh -s - $solana_version \
        --no-modify-path \
        --data-dir ./solana-install \
        --config ./solana-install/config.yml

export PATH="$PWD/solana-install/releases/$solana_version/solana-release/bin/:$PATH"

set -x
exec solana-stake-o-matic "$@"
