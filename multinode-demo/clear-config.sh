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
  rm -rf "${SOLANA_CONFIG_DIR:?}/" # <-- $i might be a symlink, rm the other side of it first
  rm -rf "$SOLANA_CONFIG_DIR"
  mkdir -p "$SOLANA_CONFIG_DIR"
)

setup_secondary_mount
