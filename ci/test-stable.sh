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


# get channel info
eval "$(ci/channel-info.sh)"

need_to_generate_test_result() {
  local branches=(
    "$EDGE_CHANNEL"
    "$BETA_CHANNEL"
    "$STABLE_CHANNEL"
  )

  for n in "${branches[@]}";
  do
    if [[ "$CI_BRANCH" == "$n" ]]; then
      return 0
    fi
  done

  return 1
}

echo "Executing $testName"
case $testName in
test-stable)
  if need_to_generate_test_result; then
    _ "$cargo" stable test --jobs "$JOBS" --all --tests --exclude solana-local-cluster ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --jobs "$JOBS" --all --tests --exclude solana-local-cluster ${V:+--verbose} -- --nocapture
  fi
  ;;
test-stable-sbf)
  # Clear the C dependency files, if dependency moves these files are not regenerated
  test -d target/debug/sbf && find target/debug/sbf -name '*.d' -delete
  test -d target/release/sbf && find target/release/sbf -name '*.d' -delete

  # rustfilt required for dumping SBF assembly listings
  "$cargo" install rustfilt

  # solana-keygen required when building C programs
  _ "$cargo" build --manifest-path=keygen/Cargo.toml

  export PATH="$PWD/target/debug":$PATH
  cargo_build_sbf="$(realpath ./cargo-build-sbf)"
  cargo_test_sbf="$(realpath ./cargo-test-sbf)"

  # SBF solana-sdk legacy compile test
  "$cargo_build_sbf" --manifest-path sdk/Cargo.toml

  # SBF C program system tests
  _ make -C programs/sbf/c tests
  if need_to_generate_test_result; then
    _ "$cargo" stable test \
      --manifest-path programs/sbf/Cargo.toml \
      --no-default-features --features=sbf_c,sbf_rust -- -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test \
      --manifest-path programs/sbf/Cargo.toml \
      --no-default-features --features=sbf_c,sbf_rust -- --nocapture
  fi

  # SBF Rust program unit tests
  for sbf_test in programs/sbf/rust/*; do
    if pushd "$sbf_test"; then
      "$cargo" test
      "$cargo_build_sbf" --sbf-sdk ../../../../sdk/sbf --dump
      "$cargo_test_sbf" --sbf-sdk ../../../../sdk/sbf
      popd
    fi
  done |& tee cargo.log
  # Save the output of cargo building the sbf tests so we can analyze
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
  if ((solana_program_count > 12)); then
      echo "Regression of build redundancy ${solana_program_count}."
      echo "Review dependency features that trigger redundant rebuilds of solana-program."
      exit 1
  fi

  # sbf-tools version
  "$cargo_build_sbf" -V

  # SBF program instruction count assertion
  sbf_target_path=programs/sbf/target
  if need_to_generate_test_result; then
    _ "$cargo" stable test \
      --manifest-path programs/sbf/Cargo.toml \
      --no-default-features --features=sbf_c,sbf_rust assert_instruction_count \
      -- -Z unstable-options --format json --report-time |& tee results.json
    awk '!/{ "type": .* }/' results.json >"${sbf_target_path}"/deploy/instuction_counts.txt
  else
    _ "$cargo" stable test \
      --manifest-path programs/sbf/Cargo.toml \
      --no-default-features --features=sbf_c,sbf_rust assert_instruction_count \
      -- --nocapture &> "${sbf_target_path}"/deploy/instuction_counts.txt
  fi

  sbf_dump_archive="sbf-dumps.tar.bz2"
  rm -f "$sbf_dump_archive"
  tar cjvf "$sbf_dump_archive" "${sbf_target_path}"/{deploy/*.txt,sbf-solana-solana/release/*.so}
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
  if need_to_generate_test_result; then
    _ "$cargo" stable test --package solana-perf --package solana-ledger --package solana-core --lib ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --package solana-perf --package solana-ledger --package solana-core --lib ${V:+--verbose} -- --nocapture
  fi
  _ "$cargo" stable run --manifest-path poh-bench/Cargo.toml ${V:+--verbose} -- --hashes-per-tick 10
  ;;
test-local-cluster)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  if need_to_generate_test_result; then
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster ${V:+--verbose} -- --nocapture --test-threads=1
  fi
  exit 0
  ;;
test-local-cluster-flakey)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  if need_to_generate_test_result; then
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_flakey ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_flakey ${V:+--verbose} -- --nocapture --test-threads=1
  fi
  exit 0
  ;;
test-local-cluster-slow-1)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  if need_to_generate_test_result; then
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_1 ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_1 ${V:+--verbose} -- --nocapture --test-threads=1
  fi
  exit 0
  ;;
test-local-cluster-slow-2)
  _ "$cargo" stable build --release --bins ${V:+--verbose}
  if need_to_generate_test_result; then
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_2 ${V:+--verbose} -- --test-threads=1 -Z unstable-options --format json --report-time | tee results.json
    exit_if_error "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --release --package solana-local-cluster --test local_cluster_slow_2 ${V:+--verbose} -- --nocapture --test-threads=1
  fi
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
  if need_to_generate_test_result; then
    _ "$cargo" stable test --jobs "$JOBS" --all --doc --exclude solana-local-cluster ${V:+--verbose} -- -Z unstable-options --format json --report-time | tee results.json
    exit "${PIPESTATUS[0]}"
  else
    _ "$cargo" stable test --jobs "$JOBS" --all --doc --exclude solana-local-cluster ${V:+--verbose} -- --nocapture
    exit 0
  fi
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
