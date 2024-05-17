#!/usr/bin/env bash
#
# Easily run the ABI tests for the entire repo or a subset
#

here=$(dirname "$0")
set -x
exec "${here}/cargo" nightly test --features frozen-abi --lib -- test_abi_ --nocapture
