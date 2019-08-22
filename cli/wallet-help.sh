#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/..

cargo build --package solana-wallet
export PATH=$PWD/target/debug:$PATH

echo "\`\`\`manpage"
solana-wallet --help
echo "\`\`\`"
echo ""

commands=(address airdrop balance cancel confirm deploy fees get-transaction-count pay send-signature send-timestamp)

for x in "${commands[@]}"; do
    echo "\`\`\`manpage"
    solana-wallet "${x}" --help
    echo "\`\`\`"
    echo ""
done
