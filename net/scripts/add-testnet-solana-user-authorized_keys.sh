#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

[[ -d /home/solana/.ssh ]] || exit 1

cd "$(dirname "$0")"

# shellcheck source=net/scripts/solana-user-authorized_keys.sh
source solana-user-authorized_keys.sh

# solana-user-authorized_keys.sh defines the public keys for users that should
# automatically be granted access to ALL testnets.
for key in "${SOLANA_PUBKEYS[@]}"; do
  echo "$key" >> /solana-authorized_keys
done

sudo -u solana bash -c "
  cat /solana-authorized_keys >> /home/solana/.ssh/authorized_keys
"
