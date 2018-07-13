#!/bin/bash -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]] || ./ci/is-pr.sh; then
  DRYRUN="echo"
fi

# BUILDKITE_TAG is the normal environment variable set by Buildkite.  However
# when this script is run from a triggered pipeline, TRIGGERED_BUILDKITE_TAG is
# used instead of BUILDKITE_TAG (due to Buildkite limitations that prevents
# BUILDKITE_TAG from propagating through to triggered pipelines)
if [[ -z "$BUILDKITE_TAG" && -z "$TRIGGERED_BUILDKITE_TAG" ]]; then
  SNAP_CHANNEL=edge
else
  SNAP_CHANNEL=beta
fi

if [[ -z $DRYRUN ]]; then
  [[ -n $SNAPCRAFT_CREDENTIALS_KEY ]] || {
    echo SNAPCRAFT_CREDENTIALS_KEY not defined
    exit 1;
  }
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

echo --- build
snapcraft

source ci/upload_ci_artifact.sh
upload_ci_artifact solana_*.snap

echo --- publish
$DRYRUN snapcraft push solana_*.snap --release $SNAP_CHANNEL
