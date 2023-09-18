#!/bin/bash

echo "about to show args: "

args=()
stake_sol=
node_sol=

# Iterate through the command-line arguments
while [ $# -gt 0 ]; do
  if [[ ${1:0:2} = -- ]]; then
    echo "first arg: $1"
    if [[ $1 = --internal-node-stake-sol ]]; then
      stake_sol=$2
      shift 2
    elif [[ $1 == --internal-node-sol ]]; then
      node_sol=$2
      shift 2
    elif [[ $1 == --tpu-enable-udp ]]; then
      args+=($1)
      shift 1
    elif [[ $1 == --tpu-disable-quic ]]; then
      args+=($1)
      shift 1
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  fi
done

for arg in "${args[@]}"; do
  echo "Argument: $arg"
done



# Check if the --internal-node-stake-sol flag was provided
if [ -z "$stake_sol" ]; then
  echo "Usage: $0 --internal-node-stake-sol <sol>"
  exit 1
fi
if [ -z "$node_sol" ]; then
  echo "Usage: $0 --internal-node-sol <sol>"
  exit 1
fi

echo "Sol stake: $stake_sol"

/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "validator"

# Maximum number of retries
MAX_RETRIES=5

# Delay between retries (in seconds)
RETRY_DELAY=1

# Solana RPC URL
SOLANA_RPC_URL="http://$BOOTSTRAP_RPC_PORT"

# Identity file
IDENTITY_FILE="identity.json"

# Function to run a Solana command with retries. need reties because sometimes dns resolver fails
# if pod dies and starts up again it may try to create a vote account or something that already exists
run_solana_command() {
    local command="$1"
    local description="$2"

    for ((retry_count=1; retry_count<=$MAX_RETRIES; retry_count++)); do
        echo "Attempt $retry_count for: $description"
        $command

        if [ $? -eq 0 ]; then
            echo "Command succeeded: $description"
            return 0
        else
            echo "Command failed for: $description (Exit status $?)"
            if [ $retry_count -lt $MAX_RETRIES ]; then
                echo "Retrying in $RETRY_DELAY seconds..."
                sleep $RETRY_DELAY
            fi
        fi
    done

    echo "Max retry limit reached. Command still failed for: $description"
    return 1
}

# Run Solana commands with retries
run_solana_command "solana -u $SOLANA_RPC_URL airdrop "$node_sol" $IDENTITY_FILE" "Airdrop"
run_solana_command "solana -u $SOLANA_RPC_URL create-vote-account --allow-unsafe-authorized-withdrawer vote.json $IDENTITY_FILE $IDENTITY_FILE -k $IDENTITY_FILE" "Create Vote Account"
run_solana_command "solana -u $SOLANA_RPC_URL create-stake-account stake.json "$stake_sol" -k $IDENTITY_FILE" "Create Stake Account"
run_solana_command "solana -u $SOLANA_RPC_URL delegate-stake stake.json vote.json --force -k $IDENTITY_FILE" "Delegate Stake"

# Check if any of the commands failed
if [ $? -ne 0 ]; then
    echo "One or more commands failed."
    exit 1
fi

echo "All commands succeeded. Running solana-validator next..."

nohup solana-validator \
  --no-os-network-limits-test \
  --identity identity.json \
  --vote-account vote.json \
  --entrypoint $BOOTSTRAP_GOSSIP_PORT \
  --rpc-faucet-address $BOOTSTRAP_FAUCET_PORT \
  --gossip-port 8001 \
  --rpc-port 8899 \
  --ledger ledger \
  --log logs/solana-validator.log \
  --full-rpc-api \
  --allow-private-addr \
  "${args[@]}" \
  >logs/init-validator.log 2>&1 &


# # Sleep for an hour (3600 seconds)
sleep 3600