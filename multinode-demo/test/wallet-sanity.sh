#!/bin/bash -e
#
# Wallet sanity test
#

here=$(dirname "$0")
cd "$here"

if [[ -n "$USE_SNAP" ]]; then
  # TODO: Merge wallet.sh functionality into solana-wallet proper and
  #       remove this USE_SNAP case
  wallet="solana.wallet $1"
else
  wallet="../wallet.sh $1"
fi

# Tokens transferred to this address are lost forever...
garbage_address=vS3ngn1TfQmpsW1Z4NkLuqNAQFF3dYQw8UZ6TCx9bmq

check_balance_output() {
  # TODO go back to a single expected_output once snaps and wallet
  #   are in sync
  #
  #   declare expected_output="$1"
  #   exec 42>&1
  #   output=$($wallet balance | tee >(cat - >&42))
  #   if [[ ! "$output" =~ $expected_output ]]; then
  #     echo "Balance is incorrect.  Expected: $expected_output"
  #     exit 1
  #   fi
  exec 42>&1
  output=$($wallet balance | tee >(cat - >&42))

  for expected_output in "$@"; do
    if [[ "$output" =~ $expected_output ]]; then
        return 0
    fi
  done
  echo "Balance is incorrect.  Expected: $expected_output"
  exit 1
}

pay_and_confirm() {
  exec 42>&1
  signature=$($wallet pay "$@" | tee >(cat - >&42))
  $wallet confirm "$signature"
}

$wallet reset
$wallet address
check_balance_output "No account found" "Your balance is: 0"
$wallet airdrop --tokens 60
check_balance_output "Your balance is: 60"
$wallet airdrop --tokens 40
check_balance_output "Your balance is: 100"
pay_and_confirm --to $garbage_address --tokens 99
check_balance_output "Your balance is: 1"

echo PASS
exit 0
