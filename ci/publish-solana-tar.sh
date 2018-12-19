#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]]; then
  DRYRUN="echo"
  CHANNEL=unknown
fi

eval "$(ci/channel-info.sh)"

if [[ $BUILDKITE_BRANCH = "$STABLE_CHANNEL" ]]; then
  CHANNEL=stable
elif [[ $BUILDKITE_BRANCH = "$EDGE_CHANNEL" ]]; then
  CHANNEL=edge
elif [[ $BUILDKITE_BRANCH = "$BETA_CHANNEL" ]]; then
  CHANNEL=beta
fi

if [[ -n "$BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$BUILDKITE_TAG
elif [[ -n "$TRIGGERED_BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$TRIGGERED_BUILDKITE_TAG
else
  CHANNEL_OR_TAG=$CHANNEL
fi

if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 1
fi


echo --- Creating tarball
(
  set -x
  rm -rf solana-release/
  mkdir solana-release/
  (
    echo "$CHANNEL_OR_TAG"
    git rev-parse HEAD
  ) > solana-release/version.txt

  scripts/cargo-install-all.sh solana-release

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh
  (
    cd fullnode
    cargo install --path . --features=cuda --root ../solana-release-cuda
  )
  cp solana-release-cuda/bin/solana-fullnode solana-release/bin/solana-fullnode-cuda

  tar jvcf solana-release.tar.bz2 solana-release/
)

echo --- Saving build artifacts
source ci/upload-ci-artifact.sh
upload-ci-artifact solana-release.tar.bz2

if [[ -n $DO_NOT_PUBLISH_TAR ]]; then
  echo Skipped due to DO_NOT_PUBLISH_TAR
  exit 0
fi

echo --- AWS S3 Store
(
  set -x
  $DRYRUN docker run \
    --rm \
    --env AWS_ACCESS_KEY_ID \
    --env AWS_SECRET_ACCESS_KEY \
    --volume "$PWD:/solana" \
    eremite/aws-cli:2018.12.18 \
    /usr/bin/s3cmd --acl-public put /solana/solana-release.tar.bz2 \
    s3://solana-release/"$CHANNEL_OR_TAG"/solana-release.tar.bz2

  echo Published to:
  $DRYRUN ci/format-url.sh http://solana-release.s3.amazonaws.com/"$CHANNEL_OR_TAG"/solana-release.tar.bz2
)
