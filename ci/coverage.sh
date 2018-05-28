#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/docker-run.sh evilmachines/rust-cargo-kcov \
  bash -exc "\
    export RUST_BACKTRACE=1; \
    cargo build --verbose; \
    cargo kcov --lib --verbose; \
  "

echo Coverage report:
ls -l target/cov/index.html

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
else
  bash <(curl -s https://codecov.io/bash)
fi

exit 0
