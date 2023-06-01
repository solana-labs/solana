#!/usr/bin/env bash

here="$(dirname "$0")"

#shellcheck source=ci/_
source "$here"/../_

#shellcheck source=ci/upload-ci-artifact.sh
source "$here"/../upload-ci-artifact.sh

#shellcheck source=ci/rust-version.sh
source "$here"/../rust-version.sh nightly

export RUST_BACKTRACE=1

export UPLOAD_METRICS=""
export TARGET_BRANCH=$CI_BRANCH
if [[ -z $CI_BRANCH ]] || [[ -n $CI_PULL_REQUEST ]]; then
  TARGET_BRANCH=$EDGE_CHANNEL
else
  UPLOAD_METRICS="upload"
fi

export BENCH_FILE=bench_output.log
export BENCH_ARTIFACT=current_bench_results.log

# Ensure all dependencies are built
_ cargo +"$rust_nightly" build --release
