#!/usr/bin/env bash
set -e

installDir=$1
if [[ -z $installDir ]]; then
  installDir="$(cd "$(dirname "$0")"/..; pwd)"
fi

channel=$(
  cd "$(dirname "$0")";
  node -p '
    let p = [
      "../lib/node_modules/@solana/web3.js/package.json",
      "../@solana/web3.js/package.json",
      "../package.json"
    ].find(require("fs").existsSync);
    if (!p) throw new Error("Unable to locate solana-web3.js directory");
    require(p)["testnetDefaultChannel"]
  '
)

if [[ -n $2 ]]; then
  channel=$2
fi
[[ $channel = edge || $channel = beta ]] || {
  echo "Error: Invalid channel: $channel"
  exit 1
}

echo "Installing $channel BPF SDK into $installDir"

set -x
cd "$installDir/"
curl -L  --retry 5 --retry-delay 2 -o bpf-sdk.tar.bz2 \
  http://solana-sdk.s3.amazonaws.com/"$channel"/bpf-sdk.tar.bz2
rm -rf bpf-sdk
mkdir -p bpf-sdk
tar jxf bpf-sdk.tar.bz2
rm -f bpf-sdk.tar.bz2

cat bpf-sdk/version.txt
