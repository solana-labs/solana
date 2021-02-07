#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t safecoinlabs/rust .

read -r rustc version _ < <(docker run safecoinlabs/rust rustc --version)
[[ $rustc = rustc ]]
docker tag safecoinlabs/rust:latest safecoinlabs/rust:"$version"
docker push safecoinlabs/rust:"$version"
docker push safecoinlabs/rust:latest
