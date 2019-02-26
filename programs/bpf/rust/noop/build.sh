#!/usr/bin/env bash

set -ex

# ensure the sdk is installed
../../../../sdk/bpf/scripts/install.sh

cargo +bpf build --release --target=bpfel_unknown_unknown -v

{ { set +x; } 2>/dev/null; echo Success; }