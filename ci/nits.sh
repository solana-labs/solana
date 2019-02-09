#!/usr/bin/env bash
#
# Project nits enforced here
#
set -e

cd "$(dirname "$0")/.."
source ci/_

# Logging hygiene: Please don't print from --lib, use the `log` crate instead
declare prints=(
  'print!'
  'println!'
  'eprint!'
  'eprintln!'
)

if _ git grep "${prints[@]/#/-e }" src; then
    exit 1
fi


# Code readability: please be explicit about the type instead of using
# Default::default()
#
# Ref: https://github.com/solana-labs/solana/issues/2630
if _ git grep 'Default::default()' -- '*.rs'; then
    exit 1
fi

