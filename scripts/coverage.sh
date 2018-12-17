#!/usr/bin/env bash
#
# Runs all tests and collects code coverage
#
# Warning: this process is a little slow
#

set -e
cd "$(dirname "$0")/.."

_() {
  echo "--- $*"
  "$@"
}

: "${BUILDKITE_COMMIT:=local}"
reportName="lcov-${BUILDKITE_COMMIT:0:9}"

export RUSTFLAGS="
  -Zprofile
  -Zno-landing-pads
  -Ccodegen-units=1
  -Cinline-threshold=0
  -Coverflow-checks=off
"
export CARGO_INCREMENTAL=0
export RUST_BACKTRACE=1

echo "--- remove old coverage results"

if [[ -d target/cov ]]; then
  find target/cov -name \*.gcda -print0 | xargs -0 rm -f
fi
rm -rf target/cov/$reportName

_ cargo +nightly build --target-dir target/cov --all
_ cargo +nightly test --target-dir target/cov --lib --all

_ scripts/fetch-grcov.sh
echo "--- grcov"
./grcov target/cov/debug/deps/ > target/cov/lcov_full.info

echo "--- filter_non_local_files_from_lcov"
filter_non_local_files_from_lcov() {
  declare skip=false
  while read -r line; do
    if [[ $line =~ ^SF:/ ]]; then
      skip=true # Skip all absolute paths as these are references into ~/.cargo
    elif [[ $line =~ ^SF:(.*) ]]; then
      # Skip relative paths that don't exist
      declare file="${BASH_REMATCH[1]}"
      if [[ -r $file ]]; then
        skip=false
      else
        skip=true
      fi
    fi
    [[ $skip = true ]] || echo "$line"
  done
}

filter_non_local_files_from_lcov < target/cov/lcov_full.info > target/cov/lcov.info

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
