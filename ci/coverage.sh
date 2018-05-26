#!/bin/bash -e

cd $(dirname $0)/..

if [[ -r ~/.cargo/env ]]; then
  # Pick up local install of kcov/cargo-kcov
  source ~/.cargo/env
fi

rustc --version
cargo --version
kcov --version
cargo-kcov --version

export RUST_BACKTRACE=1
cargo build
cargo kcov --lib

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
  exit 1
fi

bash <(curl -s https://codecov.io/bash)
exit 0
