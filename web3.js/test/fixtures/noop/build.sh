#!/bin/bash -ex

make -C ../../../examples/bpf-c-noop/
cp ../../../examples/bpf-c-noop/out/noop.o .
