#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

make -C ../../../programs/sbf/c/
cp ../../../programs/sbf/c/out/noop.so .
cat noop.so noop.so noop.so > noop_large.so
cp ../../../programs/sbf/c/out/alt_bn128.so .
