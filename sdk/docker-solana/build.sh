#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"
eval "$(../../ci/channel-info.sh)"

if [[ $BUILDKITE_BRANCH = "$STABLE_CHANNEL" ]]; then
  CHANNEL=stable
elif [[ $BUILDKITE_BRANCH = "$EDGE_CHANNEL" ]]; then
  CHANNEL=edge
elif [[ $BUILDKITE_BRANCH = "$BETA_CHANNEL" ]]; then
  CHANNEL=beta
fi

if [[ -z $CHANNEL ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 1
fi

rm -rf usr/
../../ci/docker-run.sh solanalabs/rust:1.31.0 bash -c "
  set -ex
  scripts/cargo-install-all.sh --root sdk/docker-solana/usr
  scripts/install-native-programs.sh sdk/docker-solana/usr/bin/deps/ release
"
cp -f entrypoint.sh usr/bin/solana-entrypoint.sh

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
