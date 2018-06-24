#!/bin/bash -e

cd "$(dirname "$0")/.."

cargo install --force cargo-cov
cargo cov test
cargo cov report

echo Coverage report:
ls -l target/cov/report/index.html

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
else
  bash <(curl -s https://codecov.io/bash) -x 'llvm-cov gcov'
fi

exit 0
