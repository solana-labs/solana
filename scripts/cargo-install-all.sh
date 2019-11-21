#!/usr/bin/env bash
#
# |cargo install| of the top-level crate will not install binaries for
# other workspace crates or native program crates.
set -e

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [+<cargo version>] [--use-move] [--debug] <install directory>
EOF
  exit $exitcode
}

maybeRustVersion=
useMove=false
installDir=
buildVariant=release
maybeReleaseFlag=--release

while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --use-move ]]; then
      useMove=true
      shift
    elif [[ $1 = --debug ]]; then
      maybeReleaseFlag=
      buildVariant=debug
      shift
    else
      usage "Unknown option: $1"
    fi
  elif [[ ${1:0:1} = \+ ]]; then
    maybeRustVersion=$1
    shift
  else
    installDir=$1
    shift
  fi
done

if [[ -z "$installDir" ]]; then
  usage "Install directory not specified"
  exit 1
fi

installDir="$(mkdir -p "$installDir"; cd "$installDir"; pwd)"
cargo=cargo

echo "Install location: $installDir ($buildVariant)"

cd "$(dirname "$0")"/..
./fetch-perf-libs.sh

SECONDS=0

(
  set -x
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  $cargo $maybeRustVersion build $maybeReleaseFlag

  if $useMove; then
    # shellcheck disable=SC2086 # Don't want to double quote $rust_version
    $cargo $maybeRustVersion build $maybeReleaseFlag --manifest-path programs/move_loader/Cargo.toml
  fi
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
  solana-log-analyzer
  solana-net-shaper
  solana-archiver
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
  $cargo $maybeRustVersion build $maybeReleaseFlag "${binArgs[@]}"
)

mkdir -p "$installDir/bin"
for bin in "${BINS[@]}"; do
  cp -fv "target/$buildVariant/$bin" "$installDir"/bin
done

if [[ -d target/perf-libs ]]; then
  cp -a target/perf-libs "$installDir"/bin/perf-libs
fi

set -x
mkdir -p "$installDir/bin/deps"
cp -fv target/$buildVariant/deps/libsolana*program.* "$installDir/bin/deps"

echo "Done after $SECONDS seconds"
