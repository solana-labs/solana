#!/bin/bash -e

cd $(dirname $0)/..

source $HOME/.cargo/env
rustup update
export RUST_BACKTRACE=1
cargo build
cargo kcov

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
  exit 1
fi

bash <(curl -s https://codecov.io/bash)
exit 0
