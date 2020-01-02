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

echo --- AWS S3 Store
if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Skipped
else
  (
    set -x
    docker run \
      --rm \
      --env AWS_ACCESS_KEY_ID \
      --env AWS_SECRET_ACCESS_KEY \
      --volume "$PWD:/solana" \
      eremite/aws-cli:2018.12.18 \
      /usr/bin/s3cmd --acl-public put /solana/bpf-sdk.tar.bz2 \
      s3://solana-sdk/"$CHANNEL_OR_TAG"/bpf-sdk.tar.bz2
  )
fi

exit 0
