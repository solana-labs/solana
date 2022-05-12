#!/usr/bin/env bash
#
# This script is used to upload the full buildkite pipeline. The steps defined
# in the buildkite UI should simply be:
#
#   steps:
#    - command: ".buildkite/pipeline-upload.sh"
#

set -e
# NAME=$(buildkite-agent meta-data get name)
cd "$(dirname "$0")"/..
source ci/_
sudo chmod 0777 ci/buildkite-solana-private.sh

_ ci/buildkite-solana-private.sh pipeline.yml
echo +++ pipeline
cat pipeline.yml

_ buildkite-agent pipeline upload pipeline.yml
