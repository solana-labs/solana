#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

# Run the appropriate test based on entrypoint
testName=$(basename "$0" .sh)

# Skip if only the book has been modified
ci/affects-files.sh \
  \!^book/ \
|| {
  annotate --style info \
    "Skipped $testName as only book files were modified"
  exit 0
}

source ci/rust-version.sh stable

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

# Clear the C dependency files, if dependency moves these files are not regenerated
test -d target/debug/bpf && find target/debug/bpf -name '*.d' -delete
test -d target/release/bpf && find target/release/bpf -name '*.d' -delete

# Clear the BPF sysroot files, they are not automatically rebuilt
rm -rf target/xargo # Issue #3105

echo "Executing $testName"
case $testName in
test-stable)
  _ cargo +"$rust_stable" test --all --exclude solana-local-cluster ${V:+--verbose} -- --nocapture
  _ cargo +"$rust_stable" test --manifest-path bench-tps/Cargo.toml --features=move ${V:+--verbose} test_bench_tps_local_cluster_move -- --nocapture
  ;;
test-stable-perf)
  ci/affects-files.sh \
    .rs$ \
    Cargo.lock$ \
    Cargo.toml$ \
    ^ci/rust-version.sh \
    ^ci/test-stable-perf.sh \
    ^ci/test-stable.sh \
    ^ci/test-local-cluster.sh \
    ^core/build.rs \
    ^fetch-perf-libs.sh \
    ^programs/ \
    ^sdk/ \
  || {
    annotate --style info \
      "Skipped $testName as no relevant files were modified"
    exit 0
  }

  # BPF program tests
  _ make -C programs/bpf/c tests
  _ cargo +"$rust_stable" test \
    --manifest-path programs/bpf/Cargo.toml \
    --no-default-features --features=bpf_c,bpf_rust -- --nocapture

  if [[ $(uname) = Linux ]]; then
    # Enable persistence mode to keep the CUDA kernel driver loaded, avoiding a
    # lengthy and unexpected delay the first time CUDA is involved when the driver
    # is not yet loaded.
    sudo --non-interactive ./net/scripts/enable-nvidia-persistence-mode.sh

    rm -rf target/perf-libs
    ./fetch-perf-libs.sh

    # Force CUDA for solana-core unit tests
    export TEST_PERF_LIBS_CUDA=1

    # Force CUDA in ci/localnet-sanity.sh
    export SOLANA_CUDA=1
  fi

  _ cargo +"$rust_stable" build --bins ${V:+--verbose}
  _ cargo +"$rust_stable" test --package solana-chacha-cuda --package solana-perf --package solana-ledger --package solana-core --lib ${V:+--verbose} -- --nocapture
  ;;
test-move)
  ci/affects-files.sh \
    Cargo.lock$ \
    Cargo.toml$ \
    ^ci/rust-version.sh \
    ^ci/test-stable.sh \
    ^ci/test-move.sh \
    ^programs/move_loader \
    ^programs/librapay \
    ^logger/ \
    ^runtime/ \
    ^sdk/ \
  || {
    annotate --style info \
      "Skipped $testName as no relevant files were modified"
    exit 0
  }
  _ cargo +"$rust_stable" test --manifest-path programs/move_loader/Cargo.toml ${V:+--verbose} -- --nocapture
  _ cargo +"$rust_stable" test --manifest-path programs/librapay/Cargo.toml ${V:+--verbose} -- --nocapture
  exit 0
  ;;
test-local-cluster)
  _ cargo +"$rust_stable" build --release --bins ${V:+--verbose}
  _ cargo +"$rust_stable" test --release --package solana-local-cluster ${V:+--verbose} -- --nocapture --test-threads=1
  exit 0
  ;;
*)
  echo "Error: Unknown test: $testName"
  ;;
esac

(
  export CARGO_TOOLCHAIN=+"$rust_stable"
  echo --- ci/localnet-sanity.sh
  ci/localnet-sanity.sh -x

  echo --- ci/run-sanity.sh
  ci/run-sanity.sh -x
)
