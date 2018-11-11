#!/usr/bin/env bash -ex

cd "$(dirname "$0")"
eval "$(../channel-info.sh)"

if [[ $BUILDKITE_BRANCH = "$STABLE_CHANNEL" ]]; then
  CHANNEL=stable
elif [[ $BUILDKITE_BRANCH = "$EDGE_CHANNEL" ]]; then
  CHANNEL=edge
elif [[ $BUILDKITE_BRANCH = "$BETA_CHANNEL" ]]; then
  CHANNEL=beta
fi

if [[ -z $CHANNEL ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 0
fi

rm -rf usr/
../docker-run.sh solanalabs/rust:1.30.0 \
  cargo install --path . --root ci/docker-solana/usr
cp -f entrypoint.sh usr/bin/solana-entrypoint.sh
../../scripts/install-native-programs.sh usr/bin/

docker build -t solanalabs/solana:$CHANNEL .

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
$maybeEcho docker push solanalabs/solana:$CHANNEL
