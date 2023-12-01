#!/bin/bash

# /home/solana/k8s-cluster-scripts/decode-accounts.sh -t "validator"
mkdir -p /home/solana/logs

# Start Validator
# shellcheck disable=SC1091
source /home/solana/k8s-cluster-scripts/common.sh

args=(
  --max-genesis-archive-unpacked-size 1073741824
  --no-poh-speed-test
  --no-os-network-limits-test
)
airdrops_enabled=1
node_sol=5000 # 500 SOL: number of SOL to airdrop the node for transaction fees and vote account rent exemption (ignored if airdrops_enabled=0)
stake_sol=1000
identity=validator-accounts/identity.json
vote_account=validator-accounts/vote.json
no_restart=0
gossip_entrypoint=$BOOTSTRAP_GOSSIP_ADDRESS
# gossip_entrypoint=$LOAD_BALANCER_GOSSIP_ADDRESS
ledger_dir=/home/solana/ledger
# faucet_address=$BOOTSTRAP_FAUCET_ADDRESS
faucet_address=$LOAD_BALANCER_FAUCET_ADDRESS

args+=("--entrypoint" "$gossip_entrypoint") # add in bootstrap entrypoint

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

echo "pre positional args"
positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
      shift
    elif [[ $1 == --internal-node-stake-sol ]]; then
      stake_sol=$2
      shift 2
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
    elif [[ $1 = --authorized-voter ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --authorized-withdrawer ]]; then
      authorized_withdrawer=$2
      shift 2
    elif [[ $1 = --vote-account ]]; then
      vote_account=$2
      args+=("$1" "$2")
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
  if [[ -z $vote_account ]]; then
    usage "Error: --vote-account not specified"
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

echo "gossip entrypoint: $gossip_entrypoint"

echo "pre default args"
default_arg --entrypoint "$gossip_entrypoint"
if ((airdrops_enabled)); then
  default_arg --rpc-faucet-address "$faucet_address"
fi
echo "post entrypoint arg"

default_arg --identity "$identity"
default_arg --vote-account "$vote_account"
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

# Maximum number of retries
MAX_RETRIES=30

# Delay between retries (in seconds)
RETRY_DELAY=5

# Bootstrap validator RPC URL
BOOTSTRAP_RPC_URL="http://$BOOTSTRAP_RPC_ADDRESS"

# Load balancer RPC URL
LOAD_BALANCER_RPC_URL="http://$LOAD_BALANCER_RPC_ADDRESS"

# Identity file
IDENTITY_FILE=$identity

vote_account_already_exists=false

# Function to run a Solana command with retries. need reties because sometimes dns resolver fails
# if pod dies and starts up again it may try to create a vote account or something that already exists
run_solana_command() {
    local command="$1"
    local description="$2"

    for ((retry_count = 1; retry_count <= MAX_RETRIES; retry_count++)); do
      echo "Attempt $retry_count for: $description"

      # Capture both stdout and stderr in $output
      output=$($command 2>&1)
      status=$?

      if [ $status -eq 0 ]; then
          echo "Command succeeded: $description"
          return 0
      else
        echo "Command failed for: $description (Exit status $status)"
        echo "$output" # Print the output which includes the error

        # Check for specific error message
        if [[ "$output" == *"Vote account"*"already exists"* ]]; then
            echo "Vote account already exists. Continuing without exiting."
            vote_account_already_exists=true
            return 0
        fi

        if [ "$retry_count" -lt $MAX_RETRIES ]; then
          echo "Retrying in $RETRY_DELAY seconds..."
          sleep $RETRY_DELAY
        fi
      fi
    done

    echo "Max retry limit reached. Command still failed for: $description"
    return 1
}


# Run Solana commands with retries
if ! run_solana_command "solana -u $LOAD_BALANCER_RPC_URL airdrop $node_sol $IDENTITY_FILE" "Airdrop"; then
  echo "Aidrop command failed."
  exit 1
fi

if ! run_solana_command "solana -u $LOAD_BALANCER_RPC_URL create-vote-account --allow-unsafe-authorized-withdrawer validator-accounts/vote.json $IDENTITY_FILE $IDENTITY_FILE -k $IDENTITY_FILE" "Create Vote Account"; then
  if $vote_account_already_exists; then
    echo "Vote account already exists. Skipping remaining commands."
  else
    echo "Create vote account failed."
    exit 1
  fi
fi

if [ "$vote_account_already_exists" != true ]; then
  if ! run_solana_command "solana -u $LOAD_BALANCER_RPC_URL create-stake-account validator-accounts/stake.json $stake_sol -k $IDENTITY_FILE" "Create Stake Account"; then
    echo "Create stake account failed."
    exit 1
  fi

  if ! run_solana_command "solana -u $LOAD_BALANCER_RPC_URL delegate-stake validator-accounts/stake.json validator-accounts/vote.json --force -k $IDENTITY_FILE" "Delegate Stake"; then
    echo "Delegate stake command failed."
    exit 1
  fi
fi

# sleep 3600

# $program "${args[@]}" &

# sleep 3600

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
      echo "############## validator exited, restarting ##############"
      break
    fi
    sleep 1
  done

  kill_node
done
