#!/usr/bin/env bash

# shellcheck disable=SC2086
# shellcheck disable=SC2068
# shellcheck disable=SC2206
# shellcheck disable=SC2162
# shellcheck disable=SC2178
# shellcheck disable=SC2145

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

set -e

function get_signers_string() {
  sign_only_output="$1"

  signers=()
  while read LINE; do
    signers+=( --signer $LINE )
  done <<<"$(sed -Ee $'s/^  ([a-km-zA-HJ-NP-Z1-9]{32,44}=[a-km-zA-HJ-NP-Z1-9]{64,88})$/\\1/\nt\nd' <<<"$sign_only_output")"

  echo "${signers[@]}"
}

if [[ -n "$1" ]]; then
  url="$1"
fi

if [[ -z "$url" ]]; then
  echo Provide complete URL, ex: "$0" http://devnet.solana.com:8899
  exit 1
fi
solana config set --url $url

# Create a dummy keypair file with no balance for operations that require a "client keypair file" to exist even if they don't touch it
dummy_keypair=dummy.json
solana-keygen new -o "$dummy_keypair" --no-passphrase --force --silent
solana config set --keypair $dummy_keypair

### Offline stake account creation

# The nonce account and the system account funding its creation are online
online_nonce_account_keypair=nonce_keypair.json
online_system_account_keypair=online_system_account_keypair.json

solana-keygen new -o "$online_system_account_keypair" --no-passphrase --force --silent
solana-keygen new -o "$online_nonce_account_keypair" --no-passphrase --force --silent

online_system_account_pubkey="$(solana-keygen pubkey $online_system_account_keypair)"
nonce_account_pubkey="$(solana-keygen pubkey $online_nonce_account_keypair)"

# System account funding the stake account is offline, and the auth staker and withdrawer keypairs are offline
offline_system_account_keypair=offline_system_account_keypair.json
offline_staker_keypair=offline_staker_keypair.json
offline_withdrawer_keypair=offline_withdrawer_keypair.json
offline_custodian_keypair=offline_custodian_keypair.json

solana-keygen new -o "$offline_system_account_keypair" --no-passphrase --force --silent
solana-keygen new -o "$offline_staker_keypair" --no-passphrase --force --silent
solana-keygen new -o "$offline_withdrawer_keypair" --no-passphrase --force --silent
solana-keygen new -o "$offline_custodian_keypair" --no-passphrase --force --silent

offline_system_account_pubkey="$(solana-keygen pubkey $offline_system_account_keypair)"
offline_withdrawer_pubkey="$(solana-keygen pubkey $offline_withdrawer_keypair)"
offline_staker_pubkey="$(solana-keygen pubkey $offline_staker_keypair)"
offline_custodian_pubkey="$(solana-keygen pubkey $offline_custodian_keypair)"

# Airdrop some funds to the offline account.
solana airdrop 100 $offline_system_account_pubkey
solana airdrop 2 $online_system_account_pubkey

# Create a nonce account funded by the online account, with the authority given to the offline account
solana create-nonce-account $online_nonce_account_keypair 1 --nonce-authority $offline_system_account_pubkey --keypair $online_system_account_keypair
nonce="$(solana nonce $nonce_account_pubkey)"

execution_step OFFLINE SYSTEM ACCOUNT BALANCE BEFORE CREATING STAKE ACCOUNTS
(
  set -x
  solana balance $offline_system_account_pubkey
)

################################
execution_step CREATE OFFLINE STAKE ACCOUNT
################################

# Create a stake account funded by the offline system account

stake_account_keypair=stake_account_keypair.json
solana-keygen new -o "$stake_account_keypair" --no-passphrase --force --silent
stake_account_address="$(solana-keygen pubkey $stake_account_keypair)"

sign_only="$(solana create-stake-account $stake_account_keypair 50 \
  --sign-only --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
  --stake-authority $offline_staker_pubkey --withdraw-authority $offline_withdrawer_pubkey \
  --custodian $offline_custodian_pubkey \
  --lockup-epoch 999 \
  --keypair $offline_system_account_keypair --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

solana create-stake-account $stake_account_keypair 50 \
  --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_pubkey \
  --stake-authority $offline_staker_pubkey --withdraw-authority $offline_withdrawer_pubkey \
  --custodian $offline_custodian_pubkey \
  --lockup-epoch 999 \
  --from $offline_system_account_pubkey --fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW STAKE ACCOUNT AFTER CREATION
(
  set -x
  solana stake-account $stake_account_address
)


execution_step VIEW OFFLINE SYSTEM ACCOUNT BALANCE AFTER CREATING FIRST STAKE ACCOUNT
(
  set -x
  solana balance $offline_system_account_pubkey
)

#####################
execution_step SPLIT STAKE OFFLINE
#####################

# Split the original stake account before delegating

split_stake_account_keypair=split_stake_account_keypair.json
solana-keygen new -o $split_stake_account_keypair --no-passphrase --force --silent
split_stake_account_address=$(solana-keygen pubkey $split_stake_account_keypair)

nonce="$(solana nonce $nonce_account_pubkey)"

sign_only="$(solana split-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
  --stake-authority $offline_staker_keypair $stake_account_address $split_stake_account_keypair 10 \
  --keypair $offline_system_account_keypair --sign-only --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

solana split-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_pubkey \
  --stake-authority $offline_staker_pubkey $stake_account_address $split_stake_account_keypair 10 \
  --fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW ORIGINAL STAKE ACCOUNT AFTER SPLITTING
(
  set -x
  solana stake-account $stake_account_address
)

execution_step VIEW NEW STAKE ACCOUNT CREATED FROM SPLITTING ORIGINAL
(
  set -x
  solana stake-account $split_stake_account_address
)

#####################
execution_step CHANGE CUSTODIAN LOCKUP
#####################

# Set the lockup epoch to 0 to allow stake to be withdrawn

nonce="$(solana nonce $nonce_account_pubkey)"

sign_only="$(solana stake-set-lockup --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
  $split_stake_account_address --custodian $offline_custodian_keypair --lockup-epoch 0 \
  --keypair $offline_system_account_keypair --sign-only --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

solana stake-set-lockup --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
  $split_stake_account_address --custodian $offline_custodian_pubkey --lockup-epoch 0 \
  --fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW SPLIT STAKE ACCOUNT AFTER CHANGING LOCKUP
(
  set -x
  solana stake-account $split_stake_account_address
)

##########################
execution_step OFFLINE STAKE WITHDRAWAL
##########################

# Withdraw the lamports from the stake account that was split off and return them to the offline system account

nonce="$(solana nonce $nonce_account_pubkey)"

sign_only="$(solana withdraw-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
   $split_stake_account_address $offline_system_account_pubkey 10 \
  --withdraw-authority $offline_withdrawer_keypair \
  --keypair $offline_system_account_keypair --sign-only --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

solana withdraw-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_pubkey \
  $split_stake_account_address $offline_system_account_pubkey 10 \
  --withdraw-authority $offline_withdrawer_pubkey \
  --fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW OFFLINE SYSTEM ACCOUNT BALANCE AFTER WITHDRAWING SPLIT STAKE
(
  set -x
  solana balance $offline_system_account_pubkey
)

##########################
execution_step OFFLINE STAKE DELEGATION
##########################

# Delegate stake from the original account to a vote account

vote_account_pubkey="$(awk '{if(NR==4) print $2}'<<<"$(solana show-validators)")"
nonce="$(solana nonce $nonce_account_pubkey)"

# Sign a stake delegation, assuming the authorized staker is held offline
sign_only="$(solana delegate-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
--stake-authority $offline_staker_keypair $stake_account_address $vote_account_pubkey \
--keypair $offline_system_account_keypair --sign-only --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

# Send the signed transaction on the cluster
solana delegate-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_pubkey \
--stake-authority $offline_staker_pubkey $stake_account_address $vote_account_pubkey \
--fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW ORIGINAL STAKE ACCOUNT AFTER DELEGATION
(
  set -x
  solana stake-account $stake_account_address
)

##########################
 execution_step OFFLINE STAKE DEACTIVATION
##########################

# Deactivate delegated stake

nonce="$(solana nonce $nonce_account_pubkey)"

# Sign a stake delegation, assuming the authorized staker is held offline
sign_only="$(solana deactivate-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_keypair \
--stake-authority $offline_staker_keypair $stake_account_address \
--keypair $offline_system_account_keypair --sign-only --url http://0.0.0.0)"

signers="$(get_signers_string "${sign_only[@]})")"

# Send the signed transaction on the cluster
solana deactivate-stake --blockhash $nonce --nonce $nonce_account_pubkey --nonce-authority $offline_system_account_pubkey \
--stake-authority $offline_staker_pubkey $stake_account_address \
--fee-payer $offline_system_account_pubkey ${signers[@]}

execution_step VIEW ORIGINAL STAKE ACCOUNT AFTER DEACTIVATION
(
   set -x
   solana stake-account $stake_account_address
)
