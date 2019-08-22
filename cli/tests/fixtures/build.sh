#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

make -C ../../../programs/bpf/c/
cp ../../../programs/bpf/c/out/noop.so .
