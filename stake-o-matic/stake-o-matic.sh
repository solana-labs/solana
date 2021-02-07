#!/usr/bin/env bash
#
# Downloads and runs the latest stake-o-matic binary
#

safecoin_version=edge
curl -sSf https://raw.githubusercontent.com/solana-labs/safecoin/v1.0.0/install/safecoin-install-init.sh \
    | sh -s - $safecoin_version \
        --no-modify-path \
        --data-dir ./safecoin-install \
        --config ./safecoin-install/config.yml

export PATH="$PWD/safecoin-install/releases/$safecoin_version/safecoin-release/bin/:$PATH"

set -x
safecoin --version
exec safecoin-stake-o-matic "$@"
