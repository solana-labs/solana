#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

make -C ../../../examples/bpf-c-noop/
cp ../../../examples/bpf-c-noop/out/noop.so .
