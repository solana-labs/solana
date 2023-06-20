#!/usr/bin/env bash

set -e

export RUSTFLAGS="-Cinstrument-coverage"
export LLVM_PROFILE_FILE="default-%p-%m.profraw"

cargo build
cargo test -p solana-banks-client

file_name=coverage-part2.profdata
llvm-profdata merge -sparse -o "$file_name" $(find . -name '*.profraw' -print0 | xargs -0)
buildkite-agent artifact upload "$file_name"
