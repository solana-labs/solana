#!/bin/bash -e

cd "$(dirname "$0")/.."

version=$(./ci/crate-version.sh)

echo --- Creating tarball
(
  set -x
  rm -rf bpf-sdk/
  mkdir bpf-sdk/
  (
    echo "$version"
    git rev-parse HEAD
  ) > bpf-sdk/version.txt

  cp -ra programs/bpf/c/* bpf-sdk/

  tar jvcf bpf-sdk.tar.bz2 bpf-sdk/
)


echo --- AWS S3 Store

set -x
if [[ ! -r s3cmd-2.0.1/s3cmd ]]; then
  rm -rf s3cmd-2.0.1.tar.gz s3cmd-2.0.1
  wget https://github.com/s3tools/s3cmd/releases/download/v2.0.1/s3cmd-2.0.1.tar.gz
  tar zxf s3cmd-2.0.1.tar.gz
fi

python ./s3cmd-2.0.1/s3cmd --acl-public put bpf-sdk.tar.bz2 \
  s3://solana-sdk/"$version"/bpf-sdk.tar.bz2

exit 0

