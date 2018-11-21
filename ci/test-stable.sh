#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

maybe_install() {
  for cmd in "$@"; do
    set +e
    "$cmd" --help > /dev/null 2>&1
    declare exitcode=$?
    set -e
    if [[ $exitcode -ne 0 ]]; then
      _ cargo install "$cmd"
    fi
  done
}

_ cargo build --all --verbose
_ cargo test --verbose --lib

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --test="$test" -- --test-threads=1
done

# Run native program tests
for program in programs/native/*; do
  echo --- "$program"
  (
    set -x
    cd "$program"
    cargo test --verbose
  )
done

# Build the HTML
export PATH=$CARGO_HOME/bin:$PATH
maybe_install mdbook
_ mdbook test
_ mdbook build

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
