#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh nightly
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

_ cargo build --verbose --features unstable
_ cargo test --verbose --features=unstable

maybe_cargo_install() {
  for cmd in "$@"; do
    set +e
    cargo "$cmd" --help > /dev/null 2>&1
    declare exitcode=$?
    set -e
    if [[ $exitcode -eq 101 ]]; then
      _ cargo install cargo-"$cmd"
    fi
  done
}

maybe_cargo_install cov

_ cargo cov clean
_ cargo cov test --lib
_ cargo cov report

echo --- Coverage report:
ls -l target/cov/report/index.html

if [[ -z "$CODECOV_TOKEN" ]]; then
  echo CODECOV_TOKEN undefined
else
  # TODO: Fix this.
  true
  #bash <(curl -s https://codecov.io/bash) -x 'llvm-cov-7 gcov'
fi
