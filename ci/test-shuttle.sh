#!/usr/bin/env bash

set -eo pipefail

source ci/_

cargo nextest run --profile ci --config-file ./nextest.toml --manifest-path="svm/Cargo.toml" --features="shuttle-test" --test concurrent_tests --jobs 1