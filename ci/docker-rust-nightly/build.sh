#!/usr/bin/env bash -ex

cd "$(dirname "$0")"

nightlyDate=${1:-$(date +%Y-%m-%d)}
docker build -t solanalabs/rust-nightly:"$nightlyDate" --build-arg date="$nightlyDate" .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
fi
$maybeEcho docker push solanalabs/rust-nightly:"$nightlyDate"
