#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_
export RUST_BACKTRACE=1
source scripts/ulimit-n.sh

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

ci/affects-files.sh \
  .rs$ \
  Cargo.lock$ \
  Cargo.toml$ \
  ci/test-stable-perf.sh \
  ci/test-stable.sh \
|| {
  annotate --style info --context test-stable-perf \
    "Stable Perf skipped as no .rs files were modified"
  exit 0
}

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

# Build and run root package library tests
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

# Run all program package tests
{
  # Run all BPF C tests
  make -C programs/bpf/c tests
  # Must be built out of band
  make -C programs/bpf/rust/noop/ all
  # Run programs package tests
  cargo test --manifest-path programs/Cargo.toml --no-default-features --features=bpf_c,bpf_rust
  # Run BPF loader package tests
  cargo test --manifest-path programs/native/bpf_loader/Cargo.toml --no-default-features --features=bpf_c,bpf_rust
}
