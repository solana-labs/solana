#!/bin/bash

# This file auto-generates some of the steps for the solana-testnet pipeline upload
# This allows us to dynamically skip the time-consuming tarball creation as needed.

# exit immediately on failure, or if an undefined variable is used
set -eu

# begin the pipeline.yml file
echo "steps:"

# Only trigger new tarball creation and upload if they will be used
if [[ -z $USE_PREBUILT_CHANNEL_TARBALL ]]; then
  echo "  - command: \"ci/publish-tarball.sh\""
  echo "  label: \"create build\""
fi
