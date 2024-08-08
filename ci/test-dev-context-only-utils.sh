#!/usr/bin/env bash

set -eo pipefail

scripts/check-dev-context-only-utils.sh check-all-targets "$@"
scripts/check-dev-context-only-utils.sh check-bins-and-lib "$@"
