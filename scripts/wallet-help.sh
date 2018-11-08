#!/bin/bash -e

cd "$(dirname "$0")"/..

cargo build
export PATH=$PWD/target/debug:$PATH

echo "\`\`\`"
solana-wallet --help
echo "\`\`\`"
echo ""

commands=(address airdrop balance cancel confirm deploy get-transaction-count pay send-signature send-timestamp)

for x in "${commands[@]}"; do
    echo "\`\`\`"
    solana-wallet "${x}" --help
    echo "\`\`\`"
    echo ""
done
