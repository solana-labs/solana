#!/bin/bash
set -e
echo "in non-voting-validator startup script!"

mkdir -p /home/solana/logs

echo "done decoding accounts. running faucet next..."
# sleep 3600

nohup solana-faucet --keypair non-voting-validator-accounts/faucet.json >logs/faucet.log 2>&1 &

echo "faucet running"

# Start Validator
# shellcheck disable=SC1091
source /home/solana/k8s-cluster-scripts/common.sh

args=(
  --max-genesis-archive-unpacked-size 1073741824
  --no-poh-speed-test
  --no-os-network-limits-test
  --no-voting
)
airdrops_enabled=1
node_sol=500 # 500 SOL: number of SOL to airdrop the node for transaction fees and vote account rent exemption (ignored if airdrops_enabled=0)
identity=non-voting-validator-accounts/identity.json
no_restart=0
gossip_entrypoint=$BOOTSTRAP_GOSSIP_ADDRESS
ledger_dir=/home/solana/ledger
# faucet_address=$BOOTSTRAP_FAUCET_ADDRESS
faucet_address=$LOAD_BALANCER_FAUCET_ADDRESS

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage: $0 [OPTIONS] [cluster entry point hostname]

Start a validator with no stake

OPTIONS:
  --ledger PATH             - store ledger under this PATH
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --node-sol SOL            - Number of SOL this node has been funded from the genesis config (default: $node_sol)
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits
  --no-airdrop              - The genesis config has an account for the node. Airdrops are not required.

EOF
  exit 1
}

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
      shift
    elif [[ $1 == --internal-node-sol ]]; then
      node_sol=$2
      shift 2
    # solana-validator options
    elif [[ $1 = --expected-genesis-hash ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --expected-shred-version ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity ]]; then
      identity=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --authorized-withdrawer ]]; then
      authorized_withdrawer=$2
      shift 2
    elif [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --ledger ]]; then
      ledger_dir=$2
      shift 2
    elif [[ $1 = --entrypoint ]]; then
      gossip_entrypoint=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-snapshot-fetch ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --no-voting ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --dev-no-sigverify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --dev-halt-at-slot ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --rpc-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --rpc-faucet-address ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --accounts ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --snapshot-interval-slots ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --maximum-snapshots-to-retain ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --limit-ledger-size ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-rocksdb-compaction ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-rpc-transaction-history ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-cpi-and-log-storage ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-extended-tx-metadata-storage ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --skip-poh-verify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --tpu-disable-quic ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --tpu-enable-udp ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --rpc-send-batch-ms ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --rpc-send-batch-size ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --log ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --known-validator ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --halt-on-known-validators-accounts-hash-mismatch ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --max-genesis-archive-unpacked-size ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --wait-for-supermajority ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --expected-bank-hash ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --accounts-db-skip-shrink ]]; then
      args+=("$1")
      shift
    elif [[ $1 == --require-tower ]]; then
      args+=("$1")
      shift
    elif [[ $1 = -h ]]; then
      usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

echo "post positional args"
if [[ "$SOLANA_GPU_MISSING" -eq 1 ]]; then
  echo "Testnet requires GPUs, but none were found!  Aborting..."
  exit 1
fi

if [[ ${#positional_args[@]} -gt 1 ]]; then
  usage "$@"
fi

if [[ -n $REQUIRE_KEYPAIRS ]]; then
  if [[ -z $identity ]]; then
    usage "Error: --identity not specified"
  fi
  if [[ -z $authorized_withdrawer ]]; then
    usage "Error: --authorized_withdrawer not specified"
  fi
fi

if [[ -n $gossip_entrypoint ]]; then
  # Prefer the --entrypoint argument if supplied...
  if [[ ${#positional_args[@]} -gt 0 ]]; then
    usage "$@"
  fi
else
  # ...but also support providing the entrypoint's hostname as the first
  #    positional argument
  entrypoint_hostname=${positional_args[0]}
  if [[ -z $entrypoint_hostname ]]; then
    gossip_entrypoint=127.0.0.1:8001
  else
    gossip_entrypoint="$entrypoint_hostname":8001
  fi
fi

default_arg --entrypoint "$gossip_entrypoint"
if ((airdrops_enabled)); then
  default_arg --rpc-faucet-address "$faucet_address"
fi

default_arg --identity "$identity"
default_arg --ledger "$ledger_dir"
default_arg --log logs/solana-validator.log
default_arg --full-rpc-api
default_arg --no-incremental-snapshots
default_arg --allow-private-addr
default_arg --gossip-port 8001
default_arg --rpc-port 8899

program=
if [[ -n $SOLANA_CUDA ]]; then
  program="solana-validator --cuda"
else
  program="solana-validator"
fi

echo "program: $program"

# set -e
PS4="$(basename "$0"): "
echo "PS4: $PS4"

pid=
kill_node() {
  # Note: do not echo anything from this function to ensure $pid is actually
  # killed when stdout/stderr are redirected
  set +ex
  if [[ -n $pid ]]; then
    declare _pid=$pid
    pid=
    kill "$_pid" || true
    wait "$_pid" || true
  fi
}

kill_node_and_exit() {
  kill_node
  exit
}

trap 'kill_node_and_exit' INT TERM ERR

echo "All commands succeeded. Running solana-validator next..."

echo "Validator Args"
for arg in "${args[@]}"; do
  echo "$arg"
done

while true; do
  echo "$PS4$program ${args[*]}"

  $program "${args[@]}" &
  pid=$!
  echo "pid: $pid"

  if ((no_restart)); then
    wait "$pid"
    exit $?
  fi

  while true; do
    if [[ -z $pid ]] || ! kill -0 "$pid"; then
      echo "############## non voting validator exited, restarting ##############"
      break
    fi
    sleep 1
  done

  kill_node
done
