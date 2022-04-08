#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

platform=()
if [[ $(uname -m) = arm64 ]]; then
  # Ref: https://blog.jaimyn.dev/how-to-build-multi-architecture-docker-images-on-an-m1-mac/#tldr
  platform+=(--platform linux/amd64)
fi

nightlyDate=${1:-$(date +%Y-%m-%d)}
docker build "${platform[@]}" -t solanalabs/rust-nightly:"$nightlyDate" --build-arg date="$nightlyDate" .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
fi
$maybeEcho docker push solanalabs/rust-nightly:"$nightlyDate"
