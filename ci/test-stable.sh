#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

source ci/rust-version.sh stable

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

# Clear the C dependency files, if dependeny moves these files are not regenerated
test -d target/debug/bpf && find target/debug/bpf -name '*.d' -delete
test -d target/release/bpf && find target/release/bpf -name '*.d' -delete

# Clear the BPF sysroot files, they are not automatically rebuilt
rm -rf target/xargo # Issue #3105

# Run the appropriate test based on entrypoint
testName=$(basename "$0" .sh)
case $testName in
test-stable)
  echo "Executing $testName"

  _ cargo +"$rust_stable" build --all ${V:+--verbose}
  _ cargo +"$rust_stable" test --all ${V:+--verbose} -- --nocapture
  ;;
test-stable-perf)
  echo "Executing $testName"

  ci/affects-files.sh \
    .rs$ \
    Cargo.lock$ \
    Cargo.toml$ \
    ^ci/test-stable-perf.sh \
    ^ci/test-stable.sh \
    ^core/build.rs \
    ^fetch-perf-libs.sh \
    ^programs/ \
    ^sdk/ \
  || {
    annotate --style info \
      "Skipped test-stable-perf as no relevant files were modified"
    exit 0
  }

  # BPF program tests
  _ make -C programs/bpf/c tests
  _ cargo +"$rust_stable" test \
    --manifest-path programs/bpf/Cargo.toml \
    --no-default-features --features=bpf_c,bpf_rust

  # Run root package tests with these features
  ROOT_FEATURES=
  if [[ $(uname) = Linux ]]; then
    # Enable persistence mode to keep the CUDA kernel driver loaded, avoiding a
    # lengthy and unexpected delay the first time CUDA is involved when the driver
    # is not yet loaded.
    sudo --non-interactive ./net/scripts/enable-nvidia-persistence-mode.sh

    rm -rf target/perf-libs
    ./fetch-perf-libs.sh
    # shellcheck source=/dev/null
    source ./target/perf-libs/env.sh
    ROOT_FEATURES=cuda
  fi

  # Run root package library tests
  _ cargo +"$rust_stable" build --all ${V:+--verbose} --features="$ROOT_FEATURES"
  _ cargo +"$rust_stable" test --manifest-path=core/Cargo.toml ${V:+--verbose} --features="$ROOT_FEATURES" -- --nocapture
  ;;
*)
  echo "Error: Unknown test: $testName"
  ;;
esac


## DEBUG to help track down ci/localnet-sanity.sh instability
echo --- ps
ps aux
echo --- netstat
netstat -n --protocol=ip --all
## END DEBUG

# Assumes target/debug is populated. Ensure last build command
# leaves target/debug in the state intended for localnet-sanity
echo --- ci/localnet-sanity.sh
(
  set -x
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh -x
)
