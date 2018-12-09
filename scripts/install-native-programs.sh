#!/usr/bin/env bash
#
# Installs native programs as |cargo install| doesn't know about them
#
set -e

here=$(dirname "$0")
SOLANA_ROOT="$(cd "$here"/..; pwd)"

installDir=$1
variant=${2:-release}

if [[ -z $installDir ]]; then
  echo Install directory not specified
  exit 1
fi

for dir in "$SOLANA_ROOT"/programs/native/*; do
  for program in echo "$SOLANA_ROOT"/target/"$variant"/deps/lib{,solana_}"$(basename "$dir")"{,_program}.{so,dylib,dll}; do
    if [[ -f $program ]]; then
      mkdir -p "$installDir"
      rm -f "$installDir/$(basename "$program")"
      cp -v "$program" "$installDir"
    fi
  done
done

