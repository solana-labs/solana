#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    echo "Error: Must provide the full path to the project to build"
    exit 1
fi
if [ ! -f "$1/Cargo.toml" ]; then
      echo "Error: Cannot find project: $1"
    exit 1
fi

cd "$1"

set -ex

cargo clean
