#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine>
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh
SOLANA_CONFIG_DIR=config-drone

if [[ -d "$SNAP" ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n "$(snapctl get mode)" ]] || exit 0

  # Select leader from the Snap configuration
  leader_address="$(snapctl get leader-address)"
  if [[ -z "$leader_address" ]]; then
    # Assume drone is running on the same node as the leader by default
    leader_address="localhost"
  fi
  leader="$leader_address"
else
  leader=${1:-${here}/..}  # Default to local solana repo
fi

[[ -f "$SOLANA_CONFIG_PRIVATE_DIR"/mint.json ]] || {
  echo "$SOLANA_CONFIG_PRIVATE_DIR/mint.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh -t leader"
  exit 1
}

rsync_leader_url=$(rsync_url "$leader")
set -ex
mkdir -p $SOLANA_CONFIG_DIR
$rsync -vPz "$rsync_leader_url"/config/leader.json $SOLANA_CONFIG_DIR/

# shellcheck disable=SC2086 # $solana_drone should not be quoted
exec $solana_drone \
  -l $SOLANA_CONFIG_DIR/leader.json < $SOLANA_CONFIG_PRIVATE_DIR/mint.json
