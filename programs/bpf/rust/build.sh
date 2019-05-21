#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    echo "Error: Must provide name of the project to build"
    exit 1
fi

./../../../sdk/bpf/rust-utils/build.sh "$PWD"/"$1"
