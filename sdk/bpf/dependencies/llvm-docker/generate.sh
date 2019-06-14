#!/usr/bin/env bash

read -r -d '' SCRIPT << 'EOM'
#!/usr/bin/env bash
set -e
PROGRAM=$(basename "$0")
SDKROOT="$(cd "$(dirname "$0")"/../..; pwd -P)"
[[ -z $V ]] || set -x
exec docker run \
  --workdir "$PWD" \
  --volume "$PWD:$PWD" \
  --volume "$SDKROOT:$SDKROOT" \
  --rm solanalabs/llvm \
  "$PROGRAM" "$@"
EOM

for program in clang clang++ llc ld.lld llvm-objdump llvm-objcopy; do
  echo "$SCRIPT" > bin/$program
done
