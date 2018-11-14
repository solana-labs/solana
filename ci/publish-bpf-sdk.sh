#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

version=$(./ci/crate-version.sh)
eval "$(ci/channel-info.sh)"

if [[ $BUILDKITE_BRANCH = "$STABLE_CHANNEL" ]]; then
  CHANNEL=stable
elif [[ $BUILDKITE_BRANCH = "$EDGE_CHANNEL" ]]; then
  CHANNEL=edge
elif [[ $BUILDKITE_BRANCH = "$BETA_CHANNEL" ]]; then
  CHANNEL=beta
fi

echo --- Creating tarball
(
  set -x
  rm -rf bpf-sdk/
  mkdir bpf-sdk/
  (
    echo "$version"
    git rev-parse HEAD
  ) > bpf-sdk/version.txt

  cp -ra programs/bpf/c/sdk/* bpf-sdk/

  tar jvcf bpf-sdk.tar.bz2 bpf-sdk/
)


echo --- AWS S3 Store
if [[ -z $CHANNEL ]]; then
  echo Skipped
else
  (
    set -x
    if [[ ! -r s3cmd-2.0.1/s3cmd ]]; then
      rm -rf s3cmd-2.0.1.tar.gz s3cmd-2.0.1
      wget https://github.com/s3tools/s3cmd/releases/download/v2.0.1/s3cmd-2.0.1.tar.gz
      tar zxf s3cmd-2.0.1.tar.gz
    fi

    python ./s3cmd-2.0.1/s3cmd --acl-public put bpf-sdk.tar.bz2 \
      s3://solana-sdk/"$CHANNEL"/bpf-sdk.tar.bz2
  )
fi

exit 0

