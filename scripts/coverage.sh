#!/usr/bin/env bash
#
# Runs all tests and collects code coverage
#
# Warning: this process is a little slow
#

if ! command -v grcov; then
  echo "Error: grcov not found.  Try |cargo install grcov|"
  exit 1
fi

if [[ ! "$(grcov --version)" =~ 0.8.[0-9] ]]; then
  echo Error: Required grcov version not installed

  echo "Installed version: $(grcov --version)"
  exit 1
fi

set -e
cd "$(dirname "$0")/.."
source ci/_

cargo="$(readlink -f "./cargo")"

: "${CI_COMMIT:=local}"
reportName="lcov-${CI_COMMIT:0:9}"

if [[ -z $1 ]]; then
  packages=(--lib --all --exclude solana-local-cluster)
else
  packages=("$@")
fi

coverageFlags=()
coverageFlags+=(-Zprofile)               # Enable coverage
coverageFlags+=("-Aincomplete_features") # Supress warnings due to frozen abi, which is harmless for it
if [[ $(uname) != Darwin ]]; then        # macOS skipped due to https://github.com/rust-lang/rust/issues/63047
  coverageFlags+=("-Clink-dead-code")    # Dead code should appear red in the report
fi
coverageFlags+=("-Ccodegen-units=1")     # Disable code generation parallelism which is unsupported under -Zprofile (see [rustc issue #51705]).
coverageFlags+=("-Cinline-threshold=0")  # Disable inlining, which complicates control flow.
coverageFlags+=("-Copt-level=0")
coverageFlags+=("-Coverflow-checks=off") # Disable overflow checks, which create unnecessary branches.

export RUSTFLAGS="${coverageFlags[*]} $RUSTFLAGS"
export CARGO_INCREMENTAL=0
export RUST_BACKTRACE=1
export RUST_MIN_STACK=8388608

echo "--- remove old coverage results"
if [[ -d target/cov ]]; then
  find target/cov -type f -name '*.gcda' -delete
fi
rm -rf target/cov/$reportName
mkdir -p target/cov

# Mark the base time for a clean room dir
touch target/cov/before-test

# Force rebuild of possibly-cached proc macro crates and build.rs because
# we always want stable coverage for them
# Don't support odd file names in our repo ever
if [[ -n $CI || -z $1 ]]; then
  # shellcheck disable=SC2046
  touch \
    $(git ls-files :**/build.rs) \
    $(git grep -l "proc-macro.*true" :**/Cargo.toml | sed 's|Cargo.toml|src/lib.rs|')
fi

#shellcheck source=ci/common/limit-threads.sh
source ci/common/limit-threads.sh

_ "$cargo" nightly test --jobs "$JOBS" --target-dir target/cov --no-run "${packages[@]}"

# most verbose log level (trace) is enabled for all solana code to make log!
# macro code green always
if RUST_LOG=solana=trace _ ci/intercept.sh "$cargo" nightly test --jobs "$JOBS" --target-dir target/cov "${packages[@]}" -- --nocapture; then
  test_status=0
else
  test_status=$?
  echo "Failed: $test_status"
  echo "^^^ +++"
  if [[ -n $CI ]]; then
    exit $test_status
  fi
fi
touch target/cov/after-test

echo "--- grcov"

# Create a clean room dir only with updated gcda/gcno files for this run,
# because our cached target dir is full of other builds' coverage files
rm -rf target/cov/tmp
mkdir -p target/cov/tmp

# Can't use a simpler construct under the condition of SC2044 and bash 3
# (macOS's default). See: https://github.com/koalaman/shellcheck/wiki/SC2044
find target/cov -type f -name '*.gcda' -newer target/cov/before-test ! -newer target/cov/after-test -print0 |
  (while IFS= read -r -d '' gcda_file; do
    gcno_file="${gcda_file%.gcda}.gcno"
    ln -sf "../../../$gcda_file" "target/cov/tmp/$(basename "$gcda_file")"
    ln -sf "../../../$gcno_file" "target/cov/tmp/$(basename "$gcno_file")"
  done)

(
  grcov_args=(
    target/cov/tmp
    --llvm
    --ignore \*.cargo\*
    --ignore \*build.rs
    --ignore bench-tps\*
    --ignore upload-perf\*
    --ignore bench-streamer\*
    --ignore local-cluster\*
  )

  set -x
  grcov "${grcov_args[@]}" -t html -o target/cov/$reportName
  grcov "${grcov_args[@]}" -t lcov -o target/cov/lcov.info

  cd target/cov
  tar zcf report.tar.gz $reportName
)

ls -l target/cov/$reportName/index.html
ln -sfT $reportName target/cov/LATEST

exit $test_status
