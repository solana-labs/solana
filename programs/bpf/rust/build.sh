#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    # Build all projects
    for project in */ ; do
        ./../../../sdk/bpf/rust-utils/build.sh "$PWD/$project"
    done
else
    # Build requested project
    ./../../../sdk/bpf/rust-utils/build.sh "$PWD/$1"
    
fi