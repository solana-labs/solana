#!/bin/bash -ex

cd "$(dirname "$0")"

docker build -t solanalabs/rust-nightly .
docker push solanalabs/rust-nightly
