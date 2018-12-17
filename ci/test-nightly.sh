#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."
source ci/upload-ci-artifact.sh

ci/version-check.sh nightly
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

# Uncomment this to run nightly test suit
# _ cargo test --all --verbose --features=unstable -- --test-threads=1

cargo_install_unless() {
  declare crate=$1
  shift

  "$@" > /dev/null 2>&1 || \
    _ cargo install "$crate"
}

cargo_install_unless cargo-cov cargo cov --help

# Generate coverage data and report via unit-test suite.
_ cargo cov clean
_ cargo cov build --all
_ cargo cov test --lib
_ cargo cov report
_ ./scripts/fetch-grcov.sh
_ ./grcov . -t lcov > lcov.info
_ genhtml -o target/cov/report-lcov --show-details --highlight --ignore-errors source --legend lcov.info

# Upload to tarballs to buildkite.
_ cd target/cov && tar -cjf cov-report.tar.bz2 report/* && cd -
_ upload-ci-artifact "target/cov/cov-report.tar.bz2"

_ cd target/cov && tar -cjf lcov-report.tar.bz2 report-lcov/* && cd -
_ upload-ci-artifact "target/cov/lcov-report.tar.bz2"

# Upload coverage files to buildkite for grcov debugging
_ cd target/cov/build && tar -cjf cov-gcda.tar.bz2 gcda/* && cd -
_ upload-ci-artifact "target/cov/build/cov-gcda.tar.bz2"

_ cd target/cov/build && tar -cjf cov-gcno.tar.bz2 gcno/* && cd -
_ upload-ci-artifact "target/cov/build/cov-gcno.tar.bz2"

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
else
  true
  # TODO: Why doesn't codecov grok our lcov files?
  #bash <(curl -s https://codecov.io/bash) -X gcov
fi
