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

BIN_CRATES=(
  drone
  genesis
  gossip
  install
  keygen
  ledger-tool
  replicator
  validator
  validator-info
  wallet
  bench-exchange
  bench-tps
)

for crate in "${BIN_CRATES[@]}"; do
  (
    set -x
    # shellcheck disable=SC2086 # Don't want to double quote $rust_version
    cargo $rust_version install --force --path "$crate" --root "$installDir" --features="$cargoFeatures"
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
