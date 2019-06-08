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
cargoFeatures="$2"
echo "Install location: $installDir"

cd "$(dirname "$0")"/..

SECONDS=0

(
  set -x
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  cargo $rust_version build --all --release --features="$cargoFeatures"
)

PROGRAMS=(
  solana-drone
  solana-genesis
  solana-gossip
  solana-install
  solana-install-init
  solana-keygen
  solana-ledger-tool
  solana-replicator
  solana-validator
  solana-wallet
  solana-bench-exchange
  solana-bench-streamer
  solana-bench-tps
)

for program in "${PROGRAMS[@]}"; do
  (
    set -x
    mkdir -p "$installDir"/bin
    cp target/release/"$program" "$installDir"/bin
  )
done

for dir in programs/*; do
  for program in echo target/release/deps/libsolana_"$(basename "$dir")".{so,dylib,dll}; do
    if [[ -f $program ]]; then
      mkdir -p "$installDir/bin/deps"
      rm -f "$installDir/bin/deps/$(basename "$program")"
      cp -v "$program" "$installDir"/bin/deps
    fi
  done
done

du -a "$installDir"
echo "Done after $SECONDS seconds"
