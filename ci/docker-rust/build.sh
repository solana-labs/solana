#!/bin/bash -ex

cd "$(dirname "$0")"

docker build -t solanalabs/rust .
docker push solanalabs/rust
