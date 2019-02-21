#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/test-pre.sh

# Run program package with these features
PROGRAM_FEATURES=bpf_c,bpf_rust

# Run all BPF C tests
_ make -C programs/bpf/c tests

# Must be built out of band
_ make -C programs/bpf/rust/noop/ all

_ cargo test --manifest-path programs/Cargo.toml --no-default-features --features="$PROGRAM_FEATURES"
_ cargo test --manifest-path programs/native/bpf_loader/Cargo.toml --no-default-features --features="$PROGRAM_FEATURES"

# Run root package tests witht these features
ROOT_FEATURES=chacha
# ROOT_FEATURES=erasure,chacha
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

# Post script assumes target/debug is populated.   Ensure last build command
# leaves target/debug in the state intended for further testing
exec ci/test-post.sh
