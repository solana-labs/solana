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

if [[ -n $1 ]]; then
  crate=--manifest-path=$1/Cargo.toml
else
  crate=--all
fi

coverageFlags=(-Zprofile)                # Enable coverage
coverageFlags+=("-Clink-dead-code")      # Dead code should appear red in the report
coverageFlags+=("-Ccodegen-units=1")     # Disable ThinLTO which corrupts debuginfo (see [rustc issue #45511]).
coverageFlags+=("-Cinline-threshold=0")  # Disable inlining, which complicates control flow.
coverageFlags+=("-Coverflow-checks=off") # Disable overflow checks, which create unnecessary branches.

export RUSTFLAGS="${coverageFlags[*]}"
export CARGO_INCREMENTAL=0
export RUST_BACKTRACE=1

echo "--- remove old coverage results"
if [[ -d target/cov ]]; then
  find target/cov -name \*.gcda -print0 | xargs -0 rm -f
fi
rm -rf target/cov/$reportName

source ci/rust-version.sh nightly
_ cargo +$rust_nightly build --target-dir target/cov "$crate"
_ cargo +$rust_nightly test --target-dir target/cov --lib "$crate"

echo "--- grcov"

_ grcov target/cov/debug/deps/ > target/cov/lcov-full.info

echo "--- filter-files-from-lcov"

# List of directories to remove from the coverage report
ignored_directories="^(bench-tps|upload-perf|bench-streamer)"

filter-files-from-lcov() {
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
