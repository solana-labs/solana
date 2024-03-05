#!/usr/bin/env bash
#
# |cargo install| of the top-level crate will not install binaries for
# other workspace crates or native program crates.
here="$(dirname "$0")"
readlink_cmd="readlink"
echo "OSTYPE IS: $OSTYPE"
if [[ $OSTYPE == darwin* ]]; then
  # Mac OS X's version of `readlink` does not support the -f option,
  # But `greadlink` does, which you can get with `brew install coreutils`
  readlink_cmd="greadlink"

  if ! command -v ${readlink_cmd} &> /dev/null
  then
    echo "${readlink_cmd} could not be found. You may need to install coreutils: \`brew install coreutils\`"
    exit 1
  fi
fi

SOLANA_ROOT="$("${readlink_cmd}" -f "${here}/..")"
cargo="${SOLANA_ROOT}/cargo"

set -e

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [+<cargo version>] [--debug] [--validator-only] [--release-with-debug] <install directory>
EOF
  exit $exitcode
}

maybeRustVersion=
installDir=
# buildProfileArg and buildProfile duplicate some information because cargo
# doesn't allow '--profile debug' but we still need to know that the binaries
# will be in target/debug
buildProfileArg='--profile release'
buildProfile='release'
validatorOnly=

while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --debug ]]; then
      buildProfileArg=      # the default cargo profile is 'debug'
      buildProfile='debug'
      shift
    elif [[ $1 = --release-with-debug ]]; then
      buildProfileArg='--profile release-with-debug'
      buildProfile='release-with-debug'
      shift
    elif [[ $1 = --validator-only ]]; then
      validatorOnly=true
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
mkdir -p "$installDir/bin/deps"

echo "Install location: $installDir ($buildProfile)"

cd "$(dirname "$0")"/..

SECONDS=0

if [[ $CI_OS_NAME = windows ]]; then
  # Limit windows to end-user command-line tools.  Full validator support is not
  # yet available on windows
  BINS=(
    cargo-build-bpf
    cargo-build-sbf
    cargo-test-bpf
    cargo-test-sbf
    solana
    solana-install
    solana-install-init
    solana-keygen
    solana-stake-accounts
    solana-test-validator
    solana-tokens
  )
else
  ./fetch-perf-libs.sh

  BINS=(
    solana
    solana-bench-tps
    solana-faucet
    solana-gossip
    solana-install
    solana-keygen
    solana-ledger-tool
    solana-log-analyzer
    solana-net-shaper
    solana-validator
    rbpf-cli
  )

  # Speed up net.sh deploys by excluding unused binaries
  if [[ -z "$validatorOnly" ]]; then
    BINS+=(
      cargo-build-bpf
      cargo-build-sbf
      cargo-test-bpf
      cargo-test-sbf
      solana-dos
      solana-install-init
      solana-stake-accounts
      solana-test-validator
      solana-tokens
      solana-watchtower
    )
  fi

  #XXX: Ensure `solana-genesis` is built LAST!
  # See https://github.com/solana-labs/solana/issues/5826
  BINS+=(solana-genesis)
fi

binArgs=()
for bin in "${BINS[@]}"; do
  binArgs+=(--bin "$bin")
done

mkdir -p "$installDir/bin"

(
  set -x
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  "$cargo" $maybeRustVersion build $buildProfileArg "${binArgs[@]}"

  # Exclude `spl-token` binary for net.sh builds
  if [[ -z "$validatorOnly" ]]; then
    # shellcheck source=scripts/spl-token-cli-version.sh
    source "$SOLANA_ROOT"/scripts/spl-token-cli-version.sh

    # the patch-related configs are needed for rust 1.69+ on Windows; see Cargo.toml
    # shellcheck disable=SC2086 # Don't want to double quote $rust_version
    "$cargo" $maybeRustVersion \
      --config 'patch.crates-io.ntapi.git="https://github.com/solana-labs/ntapi"' \
      --config 'patch.crates-io.ntapi.rev="97ede981a1777883ff86d142b75024b023f04fad"' \
      install --locked spl-token-cli --root "$installDir" $maybeSplTokenCliVersionArg
  fi
)

for bin in "${BINS[@]}"; do
  cp -fv "target/$buildProfile/$bin" "$installDir"/bin
done

if [[ -d target/perf-libs ]]; then
  cp -a target/perf-libs "$installDir"/bin/perf-libs
fi

if [[ -z "$validatorOnly" ]]; then
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  "$cargo" $maybeRustVersion build --manifest-path programs/bpf_loader/gen-syscall-list/Cargo.toml
  # shellcheck disable=SC2086 # Don't want to double quote $rust_version
  "$cargo" $maybeRustVersion run --bin gen-headers
  mkdir -p "$installDir"/bin/sdk/sbf
  cp -a sdk/sbf/* "$installDir"/bin/sdk/sbf
fi

# Add Solidity Compiler
if [[ -z "$validatorOnly" ]]; then
  base="https://github.com/hyperledger/solang/releases/download"
  version="v0.3.3"
  curlopt="-sSfL --retry 5 --retry-delay 2 --retry-connrefused"

  case $(uname -s) in
  "Linux")
    if [[ $(uname -m) == "x86_64" ]]; then
      arch="x86-64"
    else
      arch="arm64"
    fi
    # shellcheck disable=SC2086
    curl $curlopt -o "$installDir/bin/solang" $base/$version/solang-linux-$arch
    chmod 755 "$installDir/bin/solang"
    ;;
  "Darwin")
    if [[ $(uname -m) == "x86_64" ]]; then
      arch="intel"
    else
      arch="arm"
    fi
    # shellcheck disable=SC2086
    curl $curlopt -o "$installDir/bin/solang" $base/$version/solang-mac-$arch
    chmod 755 "$installDir/bin/solang"
    ;;
  *)
    # shellcheck disable=SC2086
    curl $curlopt -o "$installDir/bin/solang.exe" $base/$version/solang.exe
    ;;
  esac
fi

(
  set -x
  # deps dir can be empty
  shopt -s nullglob
  for dep in target/"$buildProfile"/deps/libsolana*program.*; do
    cp -fv "$dep" "$installDir/bin/deps"
  done
)

echo "Done after $SECONDS seconds"
echo
echo "To use these binaries:"
echo "  export PATH=\"$installDir\"/bin:\"\$PATH\""
