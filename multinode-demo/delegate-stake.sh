#!/usr/bin/env bash
#
# Delegate stake to a validator
#
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

stake_sol=10   # default number of SOL to assign as stake (10 SOL)
url=http://127.0.0.1:8899   # default RPC url

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage: $0 [OPTIONS] <SOL to stake ($stake_sol)>

Add stake to a validator

OPTIONS:
  --url   RPC_URL           - RPC URL to the cluster ($url)
  --label LABEL             - Append the given label to the configuration files, useful when running
                              multiple validators in the same workspace
  --no-airdrop              - Do not attempt to airdrop the stake
  --keypair FILE            - Keypair to fund the stake from
  --force                   - Override delegate-stake sanity checks
  --vote-account            - Path to vote-account keypair file
  --stake-account           - Path to stake-account keypair file

EOF
  exit 1
}

common_args=()
label=
airdrops_enabled=1
maybe_force=
keypair=

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --keypair || $1 = -k ]]; then
      keypair="$2"
      shift 2
    elif [[ $1 = --force ]]; then
      maybe_force=--force
      shift 1
    elif [[ $1 = --url || $1 = -u ]]; then
      url=$2
      shift 2
    elif [[ $1 = --vote-account ]]; then
      vote_account=$2
      shift 2
    elif [[ $1 = --stake-account ]]; then
      stake_account=$2
      shift 2
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
      shift
    elif [[ $1 = -h ]]; then
      usage "$@"
    else
      echo "Unknown argument: $1"
      usage
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

common_args+=(--url "$url")

if [[ ${#positional_args[@]} -gt 1 ]]; then
  usage "$@"
fi
if [[ -n ${positional_args[0]} ]]; then
  stake_sol=${positional_args[0]}
fi

VALIDATOR_KEYS_DIR=$SOLANA_CONFIG_DIR/validator$label
vote_account="${vote_account:-$VALIDATOR_KEYS_DIR/vote-account.json}"
stake_account="${stake_account:-$VALIDATOR_KEYS_DIR/stake-account.json}"

if [[ ! -f $vote_account ]]; then
  echo "Error: $vote_account not found"
  exit 1
fi

if ((airdrops_enabled)); then
  if [[ -z $keypair ]]; then
    echo "--keypair argument must be provided"
    exit 1
  fi
  $solana_cli \
    "${common_args[@]}" --keypair "$SOLANA_CONFIG_DIR/faucet.json" \
    transfer --allow-unfunded-recipient "$keypair" "$stake_sol"
fi

if [[ -n $keypair ]]; then
  common_args+=(--keypair "$keypair")
fi

if ! [[ -f "$stake_account" ]]; then
  $solana_keygen new --no-passphrase -so "$stake_account"
else
  echo "$stake_account already exists! Using it"
fi

set -x
$solana_cli "${common_args[@]}" \
  vote-account "$vote_account"
$solana_cli "${common_args[@]}" \
  create-stake-account "$stake_account" "$stake_sol"
$solana_cli "${common_args[@]}" \
  delegate-stake $maybe_force "$stake_account" "$vote_account"
$solana_cli "${common_args[@]}" stakes "$stake_account"
