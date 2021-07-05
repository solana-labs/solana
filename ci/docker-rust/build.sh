#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t solanalabs/rust .

read -r rustc version _ < <(docker run solanalabs/rust rustc --version)
[[ $rustc = rustc ]]
docker tag solanalabs/rust:latest solanalabs/rust:"$version"
docker push solanalabs/rust:"$version"
docker push solanalabs/rust:latest
