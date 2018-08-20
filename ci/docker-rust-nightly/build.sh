#!/bin/bash -ex

cd "$(dirname "$0")"

docker build -t solanalabs/rust-nightly .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
fi
$maybeEcho docker push solanalabs/rust-nightly
