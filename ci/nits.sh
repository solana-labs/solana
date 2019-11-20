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
  'net-utils/src'
  'runtime/src'
  'sdk/bpf/rust/rust-utils'
  'sdk/src'
  'programs/bpf/rust'
  'programs/stake/src'
  'programs/vote/src'
)

if _ git --no-pager grep -n --max-depth=0 "${prints[@]/#/-e }" -- "${print_free_tree[@]}"; then
    exit 1
fi


# Code readability: please be explicit about the type instead of using
# Default::default()
#
# Ref: https://github.com/solana-labs/solana/issues/2630
if _ git --no-pager grep -n 'Default::default()' -- '*.rs'; then
    exit 1
fi


# Github Issues should be used to track outstanding work items instead of
# marking up the code
#
# Ref: https://github.com/solana-labs/solana/issues/6474
#
# shellcheck disable=1001
declare useGithubIssueInsteadOf=(
  X\XX
  T\BD
  F\IXME
  #T\ODO  # TODO: Disable TODOs once all other TODOs are purged
)

if _ git --no-pager grep -n --max-depth=0 "${useGithubIssueInsteadOf[@]/#/-e }" -- '*.rs' '*.sh' '*.md'; then
    exit 1
fi

# TODO: Remove this `git grep` once TODOs are banned above
#       (this command is only used to highlight the current offenders)
_ git --no-pager grep -n --max-depth=0 "-e TODO" -- '*.rs' '*.sh' '*.md' || true
echo "^^^ +++"
# END TODO
