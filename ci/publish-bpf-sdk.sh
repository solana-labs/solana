#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."
eval "$(ci/channel-info.sh)"

echo --- Creating tarball
(
  set -x
  sdk/bpf/scripts/package.sh
  [[ -f bpf-sdk.tar.bz2 ]]
)

echo --- AWS S3 Store
if [[ -z $CHANNEL ]]; then
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
      s3://solana-sdk/"$CHANNEL"/bpf-sdk.tar.bz2
  )
fi

exit 0
