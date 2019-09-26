#!/usr/bin/env bash
#
# |cargo install| of the top-level crate will not install binaries for
# other workspace crates or native program crates.
set -e

export rust_version=
if [[ $1 =~ \+ ]]; then
  export rust_version=$1
  shift
fi

if [[ -z $1 ]]; then
  echo Install directory not specified
  exit 1
fi

installDir="$(mkdir -p "$1"; cd "$1"; pwd)"
cargo=cargo
debugBuild="$2"

buildVariant=release
maybeReleaseFlag=--release
if [[ -n "$debugBuild" ]]; then
  maybeReleaseFlag=
  buildVariant=debug
fi

echo "Install location: $installDir ($buildVariant)"

cd "$(dirname "$0")"/..
./fetch-perf-libs.sh

SECONDS=0

(
  set -x
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  $cargo $rust_version build $maybeReleaseFlag
)

BINS=(
  solana
  solana-bench-exchange
  solana-bench-tps
  solana-drone
  solana-gossip
  solana-install
  solana-install-init
  solana-keygen
  solana-ledger-tool
  solana-replicator
  solana-validator
)

#XXX: Ensure `solana-genesis` is built LAST!
# See https://github.com/solana-labs/solana/issues/5826
BINS+=(solana-genesis)

binArgs=()
for bin in "${BINS[@]}"; do
  binArgs+=(--bin "$bin")
done

(
  set -x
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  $cargo $rust_version build $maybeReleaseFlag "${binArgs[@]}"
)

mkdir -p "$installDir/bin"
for bin in "${BINS[@]}"; do
  cp -fv "target/$buildVariant/$bin" "$installDir"/bin
done

if [[ -d target/perf-libs ]]; then
  cp -a target/perf-libs "$installDir"/bin/perf-libs
fi

for dir in programs/*; do
  for program in echo target/$buildVariant/deps/libsolana_"$(basename "$dir")".{so,dylib,dll}; do
    if [[ -f $program ]]; then
      mkdir -p "$installDir/bin/deps"
      rm -f "$installDir/bin/deps/$(basename "$program")"
      cp -v "$program" "$installDir"/bin/deps
    fi
  done
done

echo "Done after $SECONDS seconds"
