#!/usr/bin/env bash

cd "$(dirname "$0")/.."

export CI_LOCAL_RUN=true

set -ex

scripts/build-downstream-projects.sh
