#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    # Clean all projects
    for project in */ ; do
        ./../../../sdk/bpf/rust/clean.sh "$PWD/$project"
    done
else
    # Clean requested project
    ./../../../sdk/bpf/rust/clean.sh "$PWD/$1"
    
fi
