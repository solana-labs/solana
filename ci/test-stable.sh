#!/usr/bin/env bash
set -eo pipefail
cd "$(dirname "$0")/.."

cargo="$(readlink -f "./cargo")"

source ci/_

annotate() {
  ${BUILDKITE:-false} && {
    buildkite-agent annotate "$@"
  }
}

# Run the appropriate test based on entrypoint
testName=$(basename "$0" .sh)

source ci/rust-version.sh stable

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"
source scripts/ulimit-n.sh

#shellcheck source=ci/common/limit-threads.sh
source ci/common/limit-threads.sh

# get channel info
eval "$(ci/channel-info.sh)"

#shellcheck source=ci/common/shared-functions.sh
source ci/common/shared-functions.sh

echo "Executing $testName"
case $testName in
test-stable)
  _ ci/intercept.sh cargo test --jobs "$JOBS" --all --tests --exclude solana-local-cluster ${V:+--verbose} -- --nocapture
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

  # Ensure the minimum supported "rust-version" matches platform tools to fail
  # quickly if users try to build with an older platform tools install
  cargo_toml=sdk/program/Cargo.toml
  source "scripts/read-cargo-variable.sh"
  crate_rust_version=$(readCargoVariable rust-version $cargo_toml)
  platform_tools_rust_version=$("$cargo_build_sbf" --version | grep rustc)
  platform_tools_rust_version=$(echo "$platform_tools_rust_version" | cut -d\  -f2) # Remove "rustc " prefix from a string like "rustc 1.68.0-dev"
  platform_tools_rust_version=$(echo "$platform_tools_rust_version" | cut -d- -f1)  # Remove "-dev" suffix from a string like "1.68.0-dev"

  if [[ $crate_rust_version != "$platform_tools_rust_version" ]]; then
    echo "Error: Update 'rust-version' field in '$cargo_toml' from $crate_rust_version to $platform_tools_rust_version"
    exit 1
  fi

  # SBF C program system tests
  _ make -C programs/sbf/c tests
  _ cargo test \
    --manifest-path programs/sbf/Cargo.toml \
    --no-default-features --features=sbf_c,sbf_rust -- --nocapture

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
  if ((solana_program_count > 18)); then
      echo "Regression of build redundancy ${solana_program_count}."
      echo "Review dependency features that trigger redundant rebuilds of solana-program."
      exit 1
  fi

  # platform-tools version
  "$cargo_build_sbf" -V

  # SBF program instruction count assertion
  sbf_target_path=programs/sbf/target
  _ cargo test \
    --manifest-path programs/sbf/Cargo.toml \
    --no-default-features --features=sbf_c,sbf_rust assert_instruction_count \
    -- --nocapture &> "${sbf_target_path}"/deploy/instuction_counts.txt

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

  _ cargo build --bins ${V:+--verbose}
  _ cargo test --package solana-perf --package solana-ledger --package solana-core --lib ${V:+--verbose} -- --nocapture
  _ cargo run --manifest-path poh-bench/Cargo.toml ${V:+--verbose} -- --hashes-per-tick 10
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
  _ cargo test --jobs "$JOBS" --all --doc --exclude solana-local-cluster ${V:+--verbose} -- --nocapture
  exit 0
  ;;
*)
  echo "Error: Unknown test: $testName"
  ;;
esac

(
  export CARGO_TOOLCHAIN=+"$rust_stable"
  export RUST_LOG="solana_metrics=warn,info,$RUST_LOG"
  echo --- ci/localnet-sanity.sh
  ci/localnet-sanity.sh -x

  echo --- ci/run-sanity.sh
  ci/run-sanity.sh -x
)
