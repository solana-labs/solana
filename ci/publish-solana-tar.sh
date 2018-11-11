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
  exit 0
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

  cargo install --root solana-release
  ./scripts/install-native-programs.sh solana-release/bin
  ./fetch-perf-libs.sh
  cargo install --features=cuda --root solana-release-cuda
  cp solana-release-cuda/bin/solana-fullnode solana-release/bin/solana-fullnode-cuda

  tar jvcf solana-release.tar.bz2 solana-release/
)

echo --- AWS S3 Store
if [[ -z $DRYRUN ]]; then
  (
    set -x
    if [[ ! -r s3cmd-2.0.1/s3cmd ]]; then
      rm -rf s3cmd-2.0.1.tar.gz s3cmd-2.0.1
      $DRYRUN wget https://github.com/s3tools/s3cmd/releases/download/v2.0.1/s3cmd-2.0.1.tar.gz
      $DRYRUN tar zxf s3cmd-2.0.1.tar.gz
    fi

    $DRYRUN python ./s3cmd-2.0.1/s3cmd --acl-public put solana-release.tar.bz2 \
      s3://solana-release/"$CHANNEL_OR_TAG"/solana-release.tar.bz2
  )
else
  echo Skipped due to DRYRUN
fi
exit 0

