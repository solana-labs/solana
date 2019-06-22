#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

[[ -d /home/solana/.ssh ]] || exit 1

# /solana-authorized_keys contains the public keys for users that should
# automatically be granted access to ALL testnets.
#
# To add an entry into this list:
# 1. Run: ssh-keygen -t ecdsa -N '' -f ~/.ssh/id-solana-testnet
# 2. Inline ~/.ssh/id-solana-testnet.pub below
cat > /solana-authorized_keys <<EOF
ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFBNwLw0i+rI312gWshojFlNw9NV7WfaKeeUsYADqOvM2o4yrO2pPw+sgW8W+/rPpVyH7zU9WVRgTME8NgFV1Vc=
ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBGqZAwAZeBl0buOMz4FpUYrtpwk1L5aGKlbd7lI8dpbSx5WVRPWCVKhWzsGMtDUIfmozdzJouk1LPyihghTDgsE=
EOF

sudo -u solana bash -c "
  cat /solana-authorized_keys >> /home/solana/.ssh/authorized_keys
"
