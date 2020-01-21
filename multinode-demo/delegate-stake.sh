#!/usr/bin/env bash
#
# Delegate stake to a validator
#
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

stake_lamports=1000000000   # default number of lamports to assign as stake (1 SOL)
url=http://127.0.0.1:8899   # default RPC url

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage: $0 [OPTIONS] <lamports to stake ($stake_lamports)>

Add stake to a validator

OPTIONS:
  --url   RPC_URL           - RPC URL to the cluster ($url)
  --label LABEL             - Append the given label to the configuration files, useful when running
                              multiple validators in the same workspace
  --no-airdrop              - Do not attempt to airdrop the stake
  --keypair FILE            - Keypair to fund the stake from
  --force                   - Override delegate-stake sanity checks

EOF
  exit 1
}

common_args=()
label=
airdrops_enabled=1
maybe_force=

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --keypair || $1 = -k ]]; then
      common_args+=("$1" "$2")
      shift 2
    elif [[ $1 = --force ]]; then
      maybe_force=--force
      shift 1
    elif [[ $1 = --url || $1 = -u ]]; then
      url=$2
      shift 2
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
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

common_args+=(--url "$url")

if [[ ${#positional_args[@]} -gt 1 ]]; then
  usage "$@"
fi
if [[ -n ${positional_args[0]} ]]; then
  stake_lamports=${positional_args[0]}
fi

config_dir="$SOLANA_CONFIG_DIR/validator$label"
vote_keypair_path="$config_dir"/vote-keypair.json
stake_keypair_path="$config_dir"/stake-keypair.json

if [[ ! -f $vote_keypair_path ]]; then
  echo "Error: $vote_keypair_path not found"
  exit 1
fi

if [[ -f $stake_keypair_path ]]; then
  echo "Error: $stake_keypair_path already exists"
  exit 1
fi

if ((airdrops_enabled)); then
  $solana_cli "${common_args[@]}" airdrop "$stake_lamports" lamports
fi

$solana_keygen new --no-passphrase -so "$stake_keypair_path"

set -x
$solana_cli "${common_args[@]}" \
  vote-account "$vote_keypair_path"
$solana_cli "${common_args[@]}" \
  create-stake-account "$stake_keypair_path" "$stake_lamports" lamports
$solana_cli "${common_args[@]}" \
  delegate-stake $maybe_force "$stake_keypair_path" "$vote_keypair_path"
$solana_cli "${common_args[@]}" stake-account "$stake_keypair_path"
