#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    echo "Error: Must provide the full path to the project to clean"
    exit 1
fi

./../../../sdk/bpf/rust-utils/clean.sh "$PWD"/"$1"
