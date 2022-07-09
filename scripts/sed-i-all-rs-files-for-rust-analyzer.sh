#!/usr/bin/env bash

set -e

# rust-analyzer doesn't support hiding noisy test calls in the call hierarchy from tests/benches
# so, here's some wild hack from ryoqun!

if [[ $1 = "doit" ]]; then
  # clone and build patched rustfmt somewhere: https://github.com/ryoqun/rustfmt/tree/wip-strip
  # cherry-pick this commit (this includes some doc comment bug workaround)
  # then run:
  #   $ RUSTFMT=path/to/rustfmt ./scripts/sed-i-all-rs-files-for-rust-analyzer.sh doit
  # this should even compiles!:
  #   $ cargo clippy --workspace --all-targets -- --allow=unused_imports --allow=dead_code --allow=unused_macros
  # it's true that we put true just for truely-aligned lines
  # shellcheck disable=SC2046 # our rust files are sanely named with no need to escape
  true &&
    (for f in $(echo $(git ls-files :**test**.rs | grep -v -E 'bin|main.rs|latest' ) $(git ls-files :local-cluster/**.rs)); do cat < /dev/null >| $f; done)
    (for f in $(git grep --files-with-matches  -F '#![cfg(test)]' :**.rs :^**/build.rs); do cat >| $f < /dev/null; done) &&
    (for f in $(git grep --files-with-matches  -F '#![feature(test)]' :**.rs :^**/build.rs); do cat >| $f < /dev/null; done) &&
    (for f in $(echo validator/src/bin/solana-test-validator.rs sdk/cargo-test-sbf/src/main.rs sdk/cargo-test-bpf/src/main.rs); do echo 'fn main() {}' >| $f; done) &&
    sed -i -r -e 's/#\[rustfmt::skip(.*)]/#[cfg(rustfmt_escaped_skip\1)]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e '/#\[cfg(not(test))\]/d' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[cfg(test)\]/#[rustfmt::skip]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[bench\]/#[rustfmt::skip]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[test\]/#[rustfmt::skip]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[tokio::test\]/#[rustfmt::skip]/g' $(git ls-files :**.rs :^**/build.rs) &&
    (for f in $(git ls-files :**.rs :^**/build.rs); do echo Stripping $f... && "$RUSTFMT" --edition 2021 $f; done) &&
    sed -i -r -e 's/^( *)#\[rustfmt::skip\]/\1#[cfg(escaped)]/g' $(git ls-files :**.rs :^**/build.rs)
elif [[ $1 = "undoit" ]]; then
  # shellcheck disable=SC2046 # our rust files are sanely named with no need to escape
  true &&
    sed -i -e 's/#\[cfg(escaped_cfg_test)\]/#[cfg(test)]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[cfg(escaped_bench)\]/#[bench]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[cfg(escaped_test)\]/#[test]/g' $(git ls-files :**.rs :^**/build.rs) &&
    sed -i -e 's/#\[cfg(escaped_tokio_test)\]/#[tokio::test]/g' $(git ls-files :**.rs :^**/build.rs)
else
  echo "usage: $0 [doit|undoit]" > /dev/stderr
  exit 1
fi
