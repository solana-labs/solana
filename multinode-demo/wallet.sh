#!/bin/bash
#
# usage: $0 <rsync network path to solana repo on leader machine>"
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# if $1 isn't host:path, something.com, or a valid local path
if [[ ${1%:} != "$1" || "$1" =~ [^.]\.[^.] || -d $1 ]]; then
  leader=$1 # interpret
  shift
else
  if [[ -d "$SNAP" ]]; then
    leader=testnet.solana.com # Default to testnet when running as a Snap
  else
    leader=$here/.. # Default to local solana repo
  fi
fi

if [[ "$1" = "reset" ]]; then
  echo Wallet resetting
  rm -rf "$SOLANA_CONFIG_CLIENT_DIR"
  exit 0
fi

rsync_leader_url=$(rsync_url "$leader")

set -e
mkdir -p "$SOLANA_CONFIG_CLIENT_DIR"
if [[ ! -r "$SOLANA_CONFIG_CLIENT_DIR"/leader.json ]]; then
  (
    set -x
    $rsync -vPz "$rsync_leader_url"/config/leader.json "$SOLANA_CONFIG_CLIENT_DIR"/
  )
fi

client_id_path="$SOLANA_CONFIG_CLIENT_DIR"/id.json
if [[ ! -r $client_id_path ]]; then
  $solana_keygen -o "$client_id_path"
fi

set -x
# shellcheck disable=SC2086 # $solana_wallet should not be quoted
exec $solana_wallet \
  -l "$SOLANA_CONFIG_CLIENT_DIR"/leader.json -k "$client_id_path" "$@"
