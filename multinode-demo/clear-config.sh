#!/usr/bin/env bash
#
# Clear the current cluster configuration
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

for i in "$SOLANA_RSYNC_CONFIG_DIR" "$SOLANA_CONFIG_DIR"; do
  echo "Cleaning $i"
  rm -rvf "${i:?}/" # <-- $i might be a symlink, rm the other side of it first
  rm -rvf "$i"
  mkdir -p "$i"
done

setup_secondary_mount
