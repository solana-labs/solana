#!/bin/bash -e
#
# Installs native programs as |cargo install| doesn't know about them
#

here=$(dirname "$0")
SOLANA_ROOT="$(cd "$here"/..; pwd)"

installDir=$1
variant=${2:-release}

if [[ -z $installDir ]]; then
  echo Install directory not specified
  exit 1
fi

if [[ ! -d $installDir ]]; then
  echo "Not a directory: $installDir"
  exit 1
fi

for dir in "$SOLANA_ROOT"/programs/native/*; do
  for program in echo "$SOLANA_ROOT"/target/"$variant"/deps/lib{,solana_}"$(basename "$dir")".{so,dylib,dll}; do
    if [[ -f $program ]]; then
      cp -v "$program" "$installDir"
    fi
  done
done

