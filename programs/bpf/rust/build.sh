#!/usr/bin/env bash

bpf_sdk=../../../sdk/bpf
./"$bpf_sdk"/rust-utils/build.sh "$PWD"/"$1"
