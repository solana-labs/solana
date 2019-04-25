#!/usr/bin/env bash
#
# Start a full node
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

# shellcheck source=multinode-demo/extra-fullnode-args.sh
source "$here"/extra-fullnode-args.sh

find_leader() {
  declare leader leader_address
  declare shift=0

  if [[ -z $1 ]]; then
    leader=$PWD                   # Default to local tree for rsync
    leader_address=127.0.0.1:8001 # Default to local leader
  elif [[ -z $2 ]]; then
    leader=$1
    leader_address=$leader:8001
    shift=1
  else
    leader=$1
    leader_address=$2
    shift=2
  fi

  echo "$leader" "$leader_address" "$shift"
}

read -r leader leader_address shift < <(find_leader "${@:1:2}")
shift "$shift"

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_fullnode_cuda
else
  program=$solana_fullnode
fi

fullnode_id_path=$SOLANA_CONFIG_DIR/fullnode-id$label.json
fullnode_vote_id_path=$SOLANA_CONFIG_DIR/fullnode-vote-id$label.json
ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger$label
accounts_config_dir=$SOLANA_CONFIG_DIR/fullnode-accounts$label

mkdir -p "$SOLANA_CONFIG_DIR"
[[ -r "$fullnode_id_path" ]] || $solana_keygen -o "$fullnode_id_path"
[[ -r "$fullnode_vote_id_path" ]] || $solana_keygen -o "$fullnode_vote_id_path"

fullnode_id=$($solana_keygen pubkey "$fullnode_id_path")
fullnode_vote_id=$($solana_keygen pubkey "$fullnode_vote_id_path")

cat <<EOF
======================[ Fullnode configuration ]======================
node id: $fullnode_id
vote id: $fullnode_vote_id
ledger: $ledger_config_dir
accounts: $accounts_config_dir
======================================================================
EOF

tune_system

rsync_url() { # adds the 'rsync://` prefix to URLs that need it
  declare url="$1"

  if [[ $url =~ ^.*:.*$ ]]; then
    # assume remote-shell transport when colon is present, use $url unmodified
    echo "$url"
    return 0
  fi

  if [[ -d $url ]]; then
    # assume local directory if $url is a valid directory, use $url unmodified
    echo "$url"
    return 0
  fi

  # Default to rsync:// URL
  echo "rsync://$url"
}

airdrop() {
  declare keypair_file=$1
  declare host=$2
  declare amount=$3

  declare address
  address=$($solana_wallet --keypair "$keypair_file" address)

  # TODO: Until https://github.com/solana-labs/solana/issues/2355 is resolved
  # a fullnode needs N lamports as its vote account gets re-created on every
  # node restart, costing it lamports
  declare retries=5

  while ! $solana_wallet --keypair "$keypair_file" --host "$host" airdrop "$amount"; do

    # TODO: Consider moving this retry logic into `solana-wallet airdrop`
    #   itself, currently it does not retry on "Connection refused" errors.
    ((retries--))
    if [[ $retries -le 0 ]]; then
        echo "Airdrop to $address failed."
        return 1
    fi
    echo "Airdrop to $address failed. Remaining retries: $retries"
    sleep 1
  done

  return 0
}

setup_vote_account() {
  declare drone_address=$1
  declare node_id_path=$2
  declare vote_id_path=$3
  declare stake=$4

  declare node_id
  node_id=$($solana_wallet --keypair "$node_id_path" address)

  declare vote_id
  vote_id=$($solana_wallet --keypair "$vote_id_path" address)

  if [[ -f "$vote_id_path".configured ]]; then
    echo "Vote account has already been configured"
  else
    airdrop "$node_id_path" "$drone_address" "$stake" || return $?

    # Fund the vote account from the node, with the node as the node_id
    $solana_wallet --keypair "$node_id_path" --host "$drone_address" \
      create-vote-account "$vote_id" "$node_id" $((stake - 1)) || return $?

    touch "$vote_id_path".configured
  fi

  $solana_wallet --keypair "$node_id_path" --host "$drone_address" show-vote-account "$vote_id"
  return 0
}

set -e
rsync_leader_url=$(rsync_url "$leader")
secs_to_next_genesis_poll=0
PS4="$(basename "$0"): "
while true; do
  set -x
  if [[ ! -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger ]]; then
    $rsync -vPr "$rsync_leader_url"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
  fi

  if [[ ! -d "$ledger_config_dir" ]]; then
    cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger/ "$ledger_config_dir"
    $solana_ledger_tool --ledger "$ledger_config_dir" verify
  fi

  trap '[[ -n $pid ]] && kill "$pid" >/dev/null 2>&1 && wait "$pid"' INT TERM ERR

  if ((stake)); then
    setup_vote_account "${leader_address%:*}" "$fullnode_id_path" "$fullnode_vote_id_path" "$stake"
  fi

  $program \
    --identity "$fullnode_id_path" \
    --voting-keypair "$fullnode_vote_id_path" \
    --vote-account "$fullnode_vote_id" \
    --network "$leader_address" \
    --ledger "$ledger_config_dir" \
    --accounts "$accounts_config_dir" \
    --rpc-drone-address "${leader_address%:*}:9900" \
    "${extra_fullnode_args[@]}" \
    > >($fullnode_logger) 2>&1 &
  pid=$!
  oom_score_adj "$pid" 1000

  set +x
  while true; do

    if ! kill -0 "$pid"; then
      wait "$pid"
      exit 0
    fi

    sleep 1

    ((poll_for_new_genesis_block)) || continue
    ((secs_to_next_genesis_poll--)) && continue

    $rsync -r "$rsync_leader_url"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR" || true
    diff -q "$SOLANA_RSYNC_CONFIG_DIR"/ledger/genesis.json "$ledger_config_dir"/genesis.json >/dev/null 2>&1 || break
    secs_to_next_genesis_poll=60

  done

  echo "############## New genesis detected, restarting fullnode ##############"
  kill "$pid" || true
  wait "$pid" || true
  rm -rf "$ledger_config_dir" "$accounts_config_dir" "$fullnode_vote_id_path".configured
  sleep 60 # give the network time to come back up

done
