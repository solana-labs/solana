#!/usr/bin/env bash -ex

cd "$(dirname "$0")"

docker build -t solanalabs/snapcraft .
docker push solanalabs/snapcraft
