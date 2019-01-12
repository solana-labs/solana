#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

# This job doesn't run within a container, try once to upgrade tooling on a
# version check failure
ci/version-check-with-upgrade.sh stable

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]]; then
  DRYRUN="echo"
fi

if ./ci/is-pr.sh; then
  DRYRUN="echo"
  CHANNEL="none (pullrequest)"
fi

eval "$(ci/channel-info.sh)"

if [[ -z $CHANNEL ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 1
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

echo --- checking for multilog
if [[ ! -x /usr/bin/multilog ]]; then
  if [[ -z $CI ]]; then
    echo "multilog not found, install with: sudo apt-get install -y daemontools"
    exit 1
  fi
  sudo apt-get install -y daemontools
fi

echo "--- build: $CHANNEL channel"
snapcraft

source ci/upload-ci-artifact.sh
upload-ci-artifact solana_*.snap

if [[ -z $DO_NOT_PUBLISH_SNAP ]]; then
  echo "--- publish: $CHANNEL channel"
  $DRYRUN snapcraft push solana_*.snap --release "$CHANNEL"
fi
