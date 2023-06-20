#!/usr/bin/env bash

set -e

RUSTFLAGS="-Cinstrument-coverage"
LLVM_PROFILE_FILE="default-%p-%m.profraw"

buildkite-agent artifact download "coverage*.profdata" .

cargo build
llvm-profdata merge -sparse -o solana.profdata $(find . -name '*.profdata' -print0 | xargs -0)

files=$(
  for file in \
    $(
      RUSTFLAGS="-C instrument-coverage" \
      cargo test --no-run --message-format=json |
        jq -r "select(.profile.test == true) | .filenames[]" |
        grep -v dSYM -
    ); do
    printf "%s %s " -object "$file"
  done
)

llvm-cov export $files --instr-profile=solana.profdata --format=lcov >lcov.info

curl -Os https://uploader.codecov.io/latest/linux/codecov
chmod +x codecov
./codecov
