#!/bin/bash -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH || $BUILDKITE_BRANCH =~ pull/* ]]; then
  DRYRUN="echo"
fi

if [[ -z "$BUILDKITE_TAG" ]]; then
  SNAP_CHANNEL=edge
else
  SNAP_CHANNEL=beta
fi

if [[ -n $SNAPCRAFT_CREDENTIALS_KEY ]]; then
  (
    openssl aes-256-cbc -d \
      -in ci/snapcraft.credentials.enc \
      -out ci/snapcraft.credentials \
      -k "$SNAPCRAFT_CREDENTIALS_KEY"

    snapcraft login --with ci/snapcraft.credentials
  ) || {
    rm -f ci/snapcraft.credentials;
    exit 1
  }
fi

set -x
snapcraft
$DRYRUN snapcraft push solana_*.snap --release $SNAP_CHANNEL
