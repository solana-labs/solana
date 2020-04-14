#!/usr/bin/env bash
#
# Runs all tests and collects code coverage
#
# Warning: this process is a little slow
#

set -e
cd "$(dirname "$0")/.."
source ci/_

: "${CI_COMMIT:=local}"
reportName="lcov-${CI_COMMIT:0:9}"

if [[ -z $1 ]]; then
  packages=( --lib --all --exclude solana-local-cluster )
else
  packages=( "$@" )
fi

coverageFlags=(-Zprofile)                # Enable coverage
coverageFlags+=("-Clink-dead-code")      # Dead code should appear red in the report
coverageFlags+=("-Ccodegen-units=1")     # Disable code generation parallelism which is unsupported under -Zprofile (see [rustc issue #51705]).
coverageFlags+=("-Cinline-threshold=0")  # Disable inlining, which complicates control flow.
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

source ci/rust-version.sh nightly

# Force rebuild of possibly-cached proc macro crates and build.rs because
# we always want stable coverage for them
# Don't support odd file names in our repo ever
if [[ -n $CI || -z $1 ]]; then
  # shellcheck disable=SC2046
  touch \
    $(git ls-files :**/build.rs) \
    $(git grep -l "proc-macro.*true" :**/Cargo.toml | sed 's|Cargo.toml|src/lib.rs|')
fi

RUST_LOG=solana=trace _ cargo +$rust_nightly test --target-dir target/cov --no-run "${packages[@]}"
if RUST_LOG=solana=trace _ cargo +$rust_nightly test --target-dir target/cov "${packages[@]}" 2> target/cov/coverage-stderr.log; then
  test_status=0
else
  test_status=$?
  echo "Failed: $test_status"
  echo "^^^ +++"
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

_ grcov target/cov/tmp > target/cov/lcov-full.info

echo "--- filter-files-from-lcov"

# List of directories to remove from the coverage report
ignored_directories="^(bench-tps|upload-perf|bench-streamer|bench-exchange)"

filter-files-from-lcov() {
  # this function is too noisy for casual bash -x
  set +x
  declare skip=false
  while read -r line; do
    if [[ $line =~ ^SF:/ ]]; then
      skip=true # Skip all absolute paths as these are references into ~/.cargo
    elif [[ $line =~ ^SF:(.*) ]]; then
      declare file="${BASH_REMATCH[1]}"
      if [[ $file =~ $ignored_directories ]]; then
        skip=true # Skip paths into ignored locations
      elif [[ -r $file ]]; then
        skip=false
      else
        skip=true # Skip relative paths that don't exist
      fi
    fi
    [[ $skip = true ]] || echo "$line"
  done
}

filter-files-from-lcov < target/cov/lcov-full.info > target/cov/lcov.info

echo "--- html report"
# ProTip: genhtml comes from |brew install lcov| or |apt-get install lcov|
genhtml --output-directory target/cov/$reportName \
  --show-details \
  --highlight \
  --ignore-errors source \
  --prefix "$PWD" \
  --legend \
  target/cov/lcov.info

(
  cd target/cov
  tar zcf report.tar.gz $reportName
)

ls -l target/cov/$reportName/index.html
ln -sfT $reportName target/cov/LATEST

exit $test_status
