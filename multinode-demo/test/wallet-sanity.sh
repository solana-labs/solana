#!/bin/bash -e
#
# Wallet sanity test
#

here=$(dirname "$0")
cd "$here"

wallet="../wallet.sh $1"

# Tokens transferred to this address are lost forever...
garbage_address=vS3ngn1TfQmpsW1Z4NkLuqNAQFF3dYQw8UZ6TCx9bmq

check_balance_output() {
  declare expected_output="$1"
  exec 42>&1
  output=$($wallet balance | tee >(cat - >&42))
  if [[ ! "$output" =~ $expected_output ]]; then
    echo "Balance is incorrect.  Expected: $expected_output"
    exit 1
  fi
}

# Ensure a fresh client configuration every time
rm -rf config-client

$wallet address
check_balance_output "No account found! Request an airdrop to get started"
$wallet airdrop --tokens 100
check_balance_output "Your balance is: 100"
$wallet pay --to $garbage_address --tokens 100
check_balance_output "Your balance is: 0"

echo PASS
exit 0
