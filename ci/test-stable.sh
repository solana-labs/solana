#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/version-check-with-upgrade.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

# Run tbe appropriate test based on entrypoint
testName=$(basename "$0" .sh)
case $testName in
test-stable)
  echo Executing $testName

  _ cargo build --all ${V:+--verbose}
  _ cargo test --all ${V:+--verbose} -- --nocapture --test-threads=1
  ;;
test-stable-perf)
  echo Executing $testName

  ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ci/test-stable-perf.sh \
  ci/test-stable.sh \
|| {
  annotate --style info \
    "Skipped tests as no relavant files were modified"
  exit 0
}

# Run program package with these features
PROGRAM_FEATURES=bpf_c,bpf_rust

# Run all BPF C tests
_ make -C programs/bpf/c tests

# Must be built out of band
_ make -C programs/bpf/rust/noop/ all

_ cargo test --manifest-path programs/Cargo.toml --no-default-features --features="$PROGRAM_FEATURES"
_ cargo test --manifest-path programs/native/bpf_loader/Cargo.toml --no-default-features --features="$PROGRAM_FEATURES"

# Run root package tests witht these features
ROOT_FEATURES=erasure,chacha
if [[ $(uname) = Darwin ]]; then
  ./build-perf-libs.sh
else
  # Enable persistence mode to keep the CUDA kernel driver loaded, avoiding a
  # lengthy and unexpected delay the first time CUDA is involved when the driver
  # is not yet loaded.
  sudo --non-interactive ./net/scripts/enable-nvidia-persistence-mode.sh

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh
  ROOT_FEATURES=$ROOT_FEATURES,cuda
fi

# Run root package library tests
_ cargo build ${V:+--verbose} --features="$ROOT_FEATURES"
_ cargo test --lib ${V:+--verbose} --features="$ROOT_FEATURES" -- --nocapture --test-threads=1

# Run root package integration tests
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  (
    export RUST_LOG="$test"=trace,$RUST_LOG
    _ cargo test ${V:+--verbose} --features="$ROOT_FEATURES" --test="$test" \
      -- --test-threads=1 --nocapture
  )
done
  ;;
*)
  echo "Error: Unknown test: $testName"
  ;;
esac

# Assumes target/debug is populated. Ensure last build command
# leaves target/debug in the state intended for localnet-sanity
echo --- ci/localnet-sanity.sh
(
  set -x
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh -x
)
