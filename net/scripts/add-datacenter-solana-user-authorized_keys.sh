#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

[[ -d /home/solana/.ssh ]] || mkdir -p /home/solana/.ssh

cd "$(dirname "$0")"

# shellcheck source=net/scripts/solana-user-authorized_keys.sh
source solana-user-authorized_keys.sh

# solana-user-authorized_keys.sh defines the public keys for users that should
# automatically be granted access to ALL datacenter nodes.
for i in "${!SOLANA_USERS[@]}"; do
  echo "environment=\"SOLANA_USER=${SOLANA_USERS[i]}\" ${SOLANA_PUBKEYS[i]}" >> /solana-authorized_keys
done

sudo -u solana mv /solana-authorized_keys /home/solana/.ssh/authorized_keys
