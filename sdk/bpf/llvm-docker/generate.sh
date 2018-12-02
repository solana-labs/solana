#!/usr/bin/env bash

read -r -d '' SCRIPT << 'EOM'
#!/usr/bin/env bash
set -e
WORKDIR=$( pwd )
SDKPATH="$( cd "$(dirname "$0")" ; pwd -P )"/../../inc
docker run --workdir /workdir --volume $WORKDIR:/workdir --volume $SDKPATH:/usr/local/include --rm solanalabs/llvm `basename "$0"` "$@"
EOM

echo "$SCRIPT" > bin/clang
echo "$SCRIPT" > bin/clang++
echo "$SCRIPT" > bin/llc
echo "$SCRIPT" > bin/llvm-objdump
