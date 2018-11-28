#!/usr/bin/env bash

read -r -d '' SCRIPT << 'EOM'
#!/usr/bin/env bash -ex
SDKPATH="$( cd "$(dirname "$0")" ; pwd -P )"/../../../..
docker run --workdir /solana_sdk --volume $SDKPATH:/solana_sdk --rm solanalabs/llvm `basename "$0"` "$@"
EOM

echo "$SCRIPT" > bin/clang
echo "$SCRIPT" > bin/clang++
echo "$SCRIPT" > bin/llc
echo "$SCRIPT" > bin/llvm-objdump
