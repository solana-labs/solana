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
  'dbg!'
)

# Parts of the tree that are expected to be print free
declare print_free_tree=(
  'core/src'
  'drone/src'
  'metrics/src'
  'netutil/src'
  'runtime/src'
  'sdk/src'
  'programs/vote_api/src'
  'programs/vote_program/src'
  'programs/stake_api/src'
  'programs/stake_program/src'
)

if _ git grep -n --max-depth=0 "${prints[@]/#/-e }" -- "${print_free_tree[@]}"; then
    exit 1
fi


# Code readability: please be explicit about the type instead of using
# Default::default()
#
# Ref: https://github.com/solana-labs/solana/issues/2630
if _ git grep -n 'Default::default()' -- '*.rs'; then
    exit 1
fi

# Let's keep a .gitignore for every crate, ensure it's got
#  /target/ in it
declare gitignores_ok=true
for i in $(git ls-files \*/Cargo.toml ); do
  dir=$(dirname "$i")
  if [[ ! -f $dir/.gitignore ]]; then
      echo 'error: nits.sh .gitnore missing for crate '"$dir" >&2
      gitignores_ok=false
  elif ! grep -q -e '^/target/$' "$dir"/.gitignore; then
      echo 'error: nits.sh "/target/" apparently missing from '"$dir"'/.gitignore' >&2
      gitignores_ok=false
  fi
done
"$gitignores_ok"
