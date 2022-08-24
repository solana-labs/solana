#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

cargo="$(readlink -f "./cargo")"

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

exit_if_error() {
  if [[ "$1" -ne 0 ]]; then
    exit "$1"
  fi
}

# Run the appropriate test based on entrypoint
testName=$(basename "$0" .sh)

source ci/rust-version.sh stable

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

# limit jobs to 4gb/thread
if [[ -f "/proc/meminfo" ]]; then
  JOBS=$(grep MemTotal /proc/meminfo | awk '{printf "%.0f", ($2 / (4 * 1024 * 1024))}')
else
  JOBS=$(sysctl hw.memsize | awk '{printf "%.0f", ($2 / (4 * 1024**3))}')
fi

NPROC=$(nproc)
JOBS=$((JOBS>NPROC ? NPROC : JOBS))


echo "Executing $testName"
case $testName in
test-stable)
  _ "$cargo" stable test --jobs "$JOBS" --all --tests --exclude solana-local-cluster ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  ;;
test-stable-bpf)
  # Clear the C dependency files, if dependency moves these files are not regenerated
  test -d target/debug/bpf && find target/debug/bpf -name '*.d' -delete
  test -d target/release/bpf && find target/release/bpf -name '*.d' -delete

  # rustfilt required for dumping BPF assembly listings
  "$cargo" install rustfilt

  # solana-keygen required when building C programs
  _ "$cargo" build --manifest-path=keygen/Cargo.toml

  export PATH="$PWD/target/debug":$PATH
  cargo_build_bpf="$(realpath ./cargo-build-bpf)"
  cargo_test_bpf="$(realpath ./cargo-test-bpf)"

  # BPF solana-sdk legacy compile test
  "$cargo_build_bpf" --manifest-path sdk/Cargo.toml

  # BPF C program system tests
  _ make -C programs/bpf/c tests
  _ "$cargo" stable test \
    --manifest-path programs/bpf/Cargo.toml \
    --no-default-features --features=bpf_c,bpf_rust -- -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"

  # BPF Rust program unit tests
  for bpf_test in programs/bpf/rust/*; do
    if pushd "$bpf_test"; then
      "$cargo" test
      "$cargo_build_bpf" --bpf-sdk ../../../../sdk/bpf --dump
      "$cargo_test_bpf" --bpf-sdk ../../../../sdk/bpf
      popd
    fi
  done |& tee cargo.log
  # Save the output of cargo building the bpf tests so we can analyze
  # the number of redundant rebuilds of dependency crates. The
  # expected number of solana-program crate compilations is 4. There
  # should be 3 builds of solana-program while 128bit crate is
  # built. These compilations are not redundant because the crate is
  # built for different target each time. An additional compilation of
  # solana-program is performed when simulation crate is built. This
  # last compiled solana-program is of different version, normally the
  # latest mainbeta release version.
  solana_program_count=$(grep -c 'solana-program v' cargo.log)
  rm -f cargo.log
  if ((solana_program_count > 10)); then
      echo "Regression of build redundancy ${solana_program_count}."
      echo "Review dependency features that trigger redundant rebuilds of solana-program."
      exit 1
  fi

  # bpf-tools version
  "$cargo_build_bpf" -V

  # BPF program instruction count assertion
  bpf_target_path=programs/bpf/target
  _ "$cargo" stable test \
    --manifest-path programs/bpf/Cargo.toml \
    --no-default-features --features=bpf_c,bpf_rust assert_instruction_count \
    -- -Z unstable-options --format json --report-time |& tee results.json
  awk '!/{ "type": .* }/' results.json > "${bpf_target_path}"/deploy/instuction_counts.txt

  bpf_dump_archive="bpf-dumps.tar.bz2"
  rm -f "$bpf_dump_archive"
  tar cjvf "$bpf_dump_archive" "${bpf_target_path}"/{deploy/*.txt,bpfel-unknown-unknown/release/*.so}
  exit 0
  ;;
test-stable-perf)
  if [[ $(uname) = Linux ]]; then
    # Enable persistence mode to keep the CUDA kernel driver loaded, avoiding a
    # lengthy and unexpected delay the first time CUDA is involved when the driver
    # is not yet loaded.
    sudo --non-interactive ./net/scripts/enable-nvidia-persistence-mode.sh || true

    rm -rf target/perf-libs
    ./fetch-perf-libs.sh

    # Force CUDA for solana-core unit tests
    export TEST_PERF_LIBS_CUDA=1

    # Force CUDA in ci/localnet-sanity.sh
    export SOLANA_CUDA=1
  fi

  _ "$cargo" stable build --bins ${V:+--verbose}
  _ "$cargo" stable test --package solana-perf --package solana-ledger --package solana-core --lib ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  _ "$cargo" stable run --manifest-path poh-bench/Cargo.toml ${V:+--verbose} -- --hashes-per-tick 10
  ;;
test-local-cluster)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  exit 0
  ;;
test-local-cluster-flakey)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_flakey ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  exit 0
  ;;
test-local-cluster-slow-1)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_1 ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  exit 0
  ;;
test-local-cluster-slow-2)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_2 ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
  exit 0
  ;;
test-wasm)
  _ node --version
  _ npm --version
  for dir in sdk/{program,}; do
    if [[ -r "$dir"/package.json ]]; then
      pushd "$dir"
      _ npm install
      _ npm test
      popd
    fi
  done
  exit 0
  ;;
test-docs)
  _ "$cargo" stable test --jobs "$JOBS" --all --doc --exclude solana-local-cluster ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
  exit_if_error "${PIPESTATUS[0]}"
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
