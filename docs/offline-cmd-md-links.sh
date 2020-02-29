#!/usr/bin/env bash

CLI_USAGE_RELPATH="../cli/usage.md"

SED_OMIT_NONMATCHING=$'\nt\nd'
SED_CMD="s:^#### solana-(.*):* [\`\\1\`](${CLI_USAGE_RELPATH}#solana-\\1):${SED_OMIT_NONMATCHING}"

grep -E '#### solana-|--signer ' src/cli/usage.md | grep -B1 -- --signer | sed -Ee "$SED_CMD"
