#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t solanalabs/snapcraft .
docker push solanalabs/snapcraft
