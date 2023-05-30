#!/usr/bin/env bash

here=$(dirname "$0")

find "$here" -name '*.test.sh' -exec {} \;
