#!/usr/bin/env bash

# SLP1 RPC Node
url=http://34.82.79.31:8899

if [[ -n $1 ]]; then
  url="$1"
fi

LAMPORTS_PER_SOL=1000000000 # 1 billion
TOTAL_ALLOWED_SOL=500000000 # 500 million

STAKE_PROGRAM_PUBKEY=Stake11111111111111111111111111111111111111
SYSTEM_PROGRAM_PUBKEY=11111111111111111111111111111111
VOTE_PROGRAM_PUBKEY=Vote111111111111111111111111111111111111111
STORAGE_PROGRAM_PUBKEY=Storage111111111111111111111111111111111111
CONFIG_PROGRAM_PUBKEY=Config1111111111111111111111111111111111111

tokenCapitalizationSol=
tokenCapitalizationLamports=

stakeAccountBalanceTotalSol=
systemAccountBalanceTotalSol=
voteAccountBalanceTotalSol=
storageAccountBalanceTotalSol=
configAccountBalanceTotalSol=

stakeAccountBalanceTotalLamports=
systemAccountBalanceTotalLamports=
voteAccountBalanceTotalLamports=
storageAccountBalanceTotalLamports=
configAccountBalanceTotalLamports=

function get_cluster_version {
  clusterVersion="$(curl -s -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' $url | jq '.result | ."solana-core" ')"
  echo Cluster software version: $clusterVersion
}

function get_token_capitalization {
  quiet="$1"

  totalSupplyLamports="$(curl -s -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTotalSupply"}' $url | jq .result)"
  totalSupplySol=$((totalSupplyLamports / LAMPORTS_PER_SOL))

  if [[ -z $quiet ]]; then
    printf "\n--- Token Capitalization ---\n"
    printf "Total token capitalization %'.f SOL\n" "$totalSupplySol"
    printf "Total token capitalization %'.f Lamports\n" "$totalSupplyLamports"
  fi

  tokenCapitalizationLamports="$totalSupplyLamports"
  tokenCapitalizationSol="$totalSupplySol"
}

function query_program_account_data {
  PROGRAM_NAME="$1"
  PROGRAM_PUBKEY="$2"

  curl -s -X POST -H "Content-Type: application/json" -d \
    '{"jsonrpc":"2.0","id":1, "method":"getProgramAccounts", "params":["'$PROGRAM_PUBKEY'"]}' $url | jq \
    > "${PROGRAM_NAME}_account_data.json"

}

function get_program_account_balance_totals {
  PROGRAM_NAME="$1"
  quiet="$2"

  accountBalancesLamports="$(cat "${PROGRAM_NAME}_account_data.json" | \
    jq '.result | .[] | .[1] | .lamports')"

  totalAccountBalancesLamports=0
  numberOfAccounts=0

  for account in ${accountBalancesLamports[@]}; do
    totalAccountBalancesLamports=$((totalAccountBalancesLamports + account))
    numberOfAccounts=$((numberOfAccounts + 1))
  done

  totalAccountBalancesSol=$((totalAccountBalancesLamports / LAMPORTS_PER_SOL))

  if [[ -z $quiet ]]; then
    printf "\n--- %s Account Balance Totals ---\n" "$PROGRAM_NAME"
    printf "Number of %s Program accounts: %'.f\n" "$PROGRAM_NAME" "$numberOfAccounts"
    printf "Total token balance in all %s accounts: %'.f SOL\n" "$PROGRAM_NAME" "$totalAccountBalancesSol"
    printf "Total token balance in all %s accounts: %'.f Lamports\n" "$PROGRAM_NAME" "$totalAccountBalancesLamports"
  fi

  case $PROGRAM_NAME in
    SYSTEM)
      systemAccountBalanceTotalSol=$totalAccountBalancesSol
      systemAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    STAKE)
      stakeAccountBalanceTotalSol=$totalAccountBalancesSol
      stakeAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    VOTE)
      voteAccountBalanceTotalSol=$totalAccountBalancesSol
      voteAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    STORAGE)
      storageAccountBalanceTotalSol=$totalAccountBalancesSol
      storageAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    CONFIG)
      configAccountBalanceTotalSol=$totalAccountBalancesSol
      configAccountBalanceTotalLamports=$totalAccountBalancesLamports
      ;;
    *)
      echo "Unknown program: $PROGRAM_NAME"
      exit 1
      ;;
  esac
}

function create_account_data_csv {
  PROGRAM_NAME="$1"
  csvFileName="${PROGRAM_NAME}_account_data.csv"

  echo "Account Pubkey,Lamports" > $csvFileName

  cat "${PROGRAM_NAME}_account_data.json" | \
    jq -r '(.result | .[]) | [.[0], (.[1] | .lamports)] | @csv' \
    >> $csvFileName

  echo "Wrote ${PROGRAM_NAME} account data to $csvFileName"
}

function test_sum_account_balances_capitalization {
  printf "\n--- Testing Token Capitalization vs Account Balances ---\n"
  grandTotalAccountBalancesSol=$((systemAccountBalanceTotalSol + stakeAccountBalanceTotalSol + voteAccountBalanceTotalSol + storageAccountBalanceTotalSol + configAccountBalanceTotalSol))
  printf "Total SOL in Token Capitalization: %'.f\n" "$tokenCapitalizationSol"
  printf "Total SOL in all Account Balances: %'.f\n" "$grandTotalAccountBalancesSol"

  if [[ "$grandTotalAccountBalancesSol" !=  "$tokenCapitalizationSol" ]]; then
    printf "ERROR: Difference between Capitalization and Account Balance Sum is %'.f SOL\n\n" "$((tokenCapitalizationSol - grandTotalAccountBalancesSol))"
  fi

  grandTotalAccountBalancesLamports=$((systemAccountBalanceTotalLamports + stakeAccountBalanceTotalLamports + voteAccountBalanceTotalLamports + storageAccountBalanceTotalLamports + configAccountBalanceTotalLamports))
  printf "Total Lamports in Token Capitalization: %'.f\n" "$tokenCapitalizationLamports"
  printf "Total Lamports in all Account Balances: %'.f\n" "$grandTotalAccountBalancesLamports"

  if [[ "$grandTotalAccountBalancesLamports" !=  "$tokenCapitalizationLamports" ]]; then
    printf "ERROR: Difference between Capitalization and Account Balance Sum is %'.f Lamports\n\n" "$((tokenCapitalizationLamports - grandTotalAccountBalancesLamports))"
  fi

}

echo "--- Querying RPC URL: $url ---"
get_cluster_version

query_program_account_data STAKE $STAKE_PROGRAM_PUBKEY
query_program_account_data SYSTEM $SYSTEM_PROGRAM_PUBKEY
query_program_account_data VOTE $VOTE_PROGRAM_PUBKEY
query_program_account_data STORAGE $STORAGE_PROGRAM_PUBKEY
query_program_account_data CONFIG $CONFIG_PROGRAM_PUBKEY

create_account_data_csv STAKE
create_account_data_csv SYSTEM
create_account_data_csv VOTE
create_account_data_csv STORAGE
create_account_data_csv CONFIG

get_token_capitalization #quiet

get_program_account_balance_totals STAKE #quiet
get_program_account_balance_totals SYSTEM #quiet
get_program_account_balance_totals VOTE #quiet
get_program_account_balance_totals STORAGE #quiet
get_program_account_balance_totals CONFIG #quiet

test_sum_account_balances_capitalization
