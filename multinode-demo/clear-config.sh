#!/usr/bin/env bash
#
# Clear the current cluster configuration
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

(
  set -x
  setup_secondary_mount
  echo "Removing config dir"
  rm -rf ${SOLANA_CONFIG_DIR}/*
)
