#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"/../..
eval "$(ci/channel-info.sh)"
source ci/rust-version.sh

CHANNEL_OR_TAG=
if [[ -n "$CI_TAG" ]]; then
  CHANNEL_OR_TAG=$CI_TAG
else
  CHANNEL_OR_TAG=$CHANNEL
fi

if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Unable to determine channel or tag to publish into, exiting.
  echo "^^^ +++"
  exit 0
fi

cd "$(dirname "$0")"
rm -rf usr/
../../ci/docker-run.sh "$rust_stable_docker_image" \
  scripts/cargo-install-all.sh sdk/docker-solana/usr

cp -f ../../run.sh usr/bin/solana-run.sh

docker build -t solanalabs/solana:"$CHANNEL_OR_TAG" .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
else
  (
    set +x
    if [[ -n $DOCKER_PASSWORD && -n $DOCKER_USERNAME ]]; then
      echo "$DOCKER_PASSWORD" | docker login --username "$DOCKER_USERNAME" --password-stdin
    fi
  )
fi
$maybeEcho docker push solanalabs/solana:"$CHANNEL_OR_TAG"
