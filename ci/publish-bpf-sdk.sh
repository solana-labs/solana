#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."
eval "$(ci/channel-info.sh)"

if [[ -n "$CI_TAG" ]]; then
  CHANNEL_OR_TAG=$CI_TAG
else
  CHANNEL_OR_TAG=$CHANNEL
fi

(
  set -x
  sdk/bpf/scripts/package.sh
  [[ -f bpf-sdk.tar.bz2 ]]
)

source ci/upload-ci-artifact.sh
echo --- AWS S3 Store
if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Skipped
else
  upload-s3-artifact "/solana/bpf-sdk.tar.bz2" "s3://solana-sdk/$CHANNEL_OR_TAG/bpf-sdk.tar.bz2"
fi

exit 0
