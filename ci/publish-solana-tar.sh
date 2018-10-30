#!/bin/bash -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]] || ./ci/is-pr.sh; then
  DRYRUN="echo"
fi

eval "$(ci/channel-info.sh)"

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

echo --- Creating tarball
if [[ -z $DRYRUN ]]; then
(
  set -x
  rm -rf solana-release/
  mkdir solana-release/
  (
    echo "$CHANNEL"
    git rev-parse HEAD
  ) > solana-release/version.txt

  cargo install --features=cuda --root solana-release
  ./scripts/install-native-programs.sh solana-release

  tar jvcf solana-release.tar.bz2 solana-release/
)
fi


echo --- AWS S3 Store

set -x
if [[ ! -r s3cmd-2.0.1/s3cmd ]]; then
  rm -rf s3cmd-2.0.1.tar.gz s3cmd-2.0.1
  $DRYRUN wget https://github.com/s3tools/s3cmd/releases/download/v2.0.1/s3cmd-2.0.1.tar.gz
  $DRYRUN tar zxf s3cmd-2.0.1.tar.gz
fi

$DRYRUN python ./s3cmd-2.0.1/s3cmd --acl-public put solana-release.tar.bz2 \
  s3://solana-release/"$CHANNEL"/solana-release.tar.bz2

exit 0

