#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

[[ -d /home/safecoin/.ssh ]] || exit 1

if [[ ${#SAFECOIN_PUBKEYS[@]} -eq 0 ]]; then
  echo "Warning: source safecoin-user-authorized_keys.sh first"
fi

# safecoin-user-authorized_keys.sh defines the public keys for users that should
# automatically be granted access to ALL testnets
for key in "${SAFECOIN_PUBKEYS[@]}"; do
  echo "$key" >> /safecoin-scratch/authorized_keys
done

sudo -u safecoin bash -c "
  cat /safecoin-scratch/authorized_keys >> /home/safecoin/.ssh/authorized_keys
"
