#!/usr/bin/env bash

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/get_program_accounts.sh

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [cluster_rpc_url]

 Report total token distribution of a running cluster owned by the following programs:
   STAKE
   SYSTEM
   VOTE
   CONFIG

 Required arguments:
   cluster_rpc_url  - RPC URL and port for a running Solana cluster (ex: http://34.83.146.144:8899)
EOF
  exit $exitcode
}

function get_cluster_version {
  clusterVersion="$(curl -s -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' "$url" | jq '.result | ."solana-core" ')"
  echo Cluster software version: "$clusterVersion"
}

function get_token_capitalization {
  totalSupplyLamports="$(curl -s -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTotalSupply"}' "$url" | cut -d , -f 2 | cut -d : -f 2)"
  totalSupplySol=$((totalSupplyLamports / LAMPORTS_PER_SAND))

  printf "\n--- Token Capitalization ---\n"
  printf "Total token capitalization %'d SAND\n" "$totalSupplySand"
  printf "Total token capitalization %'d Lamports\n" "$totalSupplyLamports"

}

function get_program_account_balance_totals {
  PROGRAM_NAME="$1"

  # shellcheck disable=SC2002
  accountBalancesLamports="$(cat "${PROGRAM_NAME}_account_data.json" | \
    jq '.result | .[] | .account | .lamports')"

  totalAccountBalancesLamports=0
  numberOfAccounts=0

  # shellcheck disable=SC2068
  for account in ${accountBalancesLamports[@]}; do
    totalAccountBalancesLamports=$((totalAccountBalancesLamports + account))
    numberOfAccounts=$((numberOfAccounts + 1))
  done
  totalAccountBalancesSand=$((totalAccountBalancesLamports / LAMPORTS_PER_SAND))

  printf "\n--- %s Account Balance Totals ---\n" "$PROGRAM_NAME"
  printf "Number of %s Program accounts: %'.f\n" "$PROGRAM_NAME" "$numberOfAccounts"
  printf "Total token balance in all %s accounts: %'d SAND\n" "$PROGRAM_NAME" "$totalAccountBalancesSand"
  printf "Total token balance in all %s accounts: %'d Lamports\n" "$PROGRAM_NAME" "$totalAccountBalancesLamports"

  case $PROGRAM_NAME in
    SYSTEM)
      systemAccountBalanceTotalSand=$totalAccountBalancesSand
      systemAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    STAKE)
      stakeAccountBalanceTotalSand=$totalAccountBalancesSand
      stakeAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    VOTE)
      voteAccountBalanceTotalSand=$totalAccountBalancesSand
      voteAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    CONFIG)
      configAccountBalanceTotalSand=$totalAccountBalancesSand
      configAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    *)
      echo "Unknown program: $PROGRAM_NAME"
      exit 1
      ;;
  esac
}

function sum_account_balances_totals {
  grandtotalAccountBalancesSand=$((systemAccountBalanceTotalSand + stakeAccountBalanceTotalSand + voteAccountBalanceTotalSand + configAccountBalanceTotalSand))
  grandTotalAccountBalancesLamports=$((systemAccountBalanceTotalLamports + stakeAccountBalanceTotalLamports + voteAccountBalanceTotalLamports + configAccountBalanceTotalLamports))

  printf "\n--- Total Token Distribution in all Account Balances ---\n"
  printf "Total SAND in all Account Balances: %'d\n" "$grandtotalAccountBalancesSand"
  printf "Total Lamports in all Account Balances: %'d\n" "$grandTotalAccountBalancesLamports"
}

url=$1
[[ -n $url ]] || usage "Missing required RPC URL"
shift

LAMPORTS_PER_SAND=1000000000 # 1 billion

stakeAccountBalanceTotalSand=
systemAccountBalanceTotalSand=
voteAccountBalanceTotalSand=
configAccountBalanceTotalSand=

stakeAccountBalanceTotalLamports=
systemAccountBalanceTotalLamports=
voteAccountBalanceTotalLamports=
configAccountBalanceTotalLamports=

echo "--- Querying RPC URL: $url ---"
get_cluster_version

get_program_accounts STAKE "$STAKE_PROGRAM_PUBKEY" "$url"
get_program_accounts SYSTEM "$SYSTEM_PROGRAM_PUBKEY" "$url"
get_program_accounts VOTE "$VOTE_PROGRAM_PUBKEY" "$url"
get_program_accounts CONFIG "$CONFIG_PROGRAM_PUBKEY" "$url"

write_program_account_data_csv STAKE
write_program_account_data_csv SYSTEM
write_program_account_data_csv VOTE
write_program_account_data_csv CONFIG

get_token_capitalization

get_program_account_balance_totals STAKE
get_program_account_balance_totals SYSTEM
get_program_account_balance_totals VOTE
get_program_account_balance_totals CONFIG

sum_account_balances_totals
