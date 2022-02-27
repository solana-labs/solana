#!/usr/bin/env bash
#
# This script is used to upload the full buildkite pipeline. The steps defined
# in the buildkite UI should simply be:
#
#   steps:
#    - command: ".buildkite/pipeline-upload.sh"
#

set -e
cd "$(dirname "$0")"/..
source ci/_
sudo chmod 0777 ci/buildkite-pipeline-in-disk.sh

_ ci/buildkite-pipeline-in-disk.sh pipeline.yml
echo +++ pipeline
cat pipeline.yml

_ buildkite-agent pipeline upload pipeline.yml
