#!/usr/bin/env bash
set -ex

make -C ../../../examples/bpf-c-noop/
cp ../../../examples/bpf-c-noop/out/noop.o .
