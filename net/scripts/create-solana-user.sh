#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

if grep -q safecoin /etc/passwd ; then
  echo "User safecoin already exists"
else
  adduser safecoin --gecos "" --disabled-password --quiet
  adduser safecoin sudo
  adduser safecoin adm
  echo "safecoin ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
  id safecoin

  [[ -r /safecoin-scratch/id_ecdsa ]] || exit 1
  [[ -r /safecoin-scratch/id_ecdsa.pub ]] || exit 1

  sudo -u safecoin bash -c "
    echo 'PATH=\"/home/safecoin/.cargo/bin:$PATH\"' > /home/safecoin/.profile
    mkdir -p /home/safecoin/.ssh/
    cd /home/safecoin/.ssh/
    cp /safecoin-scratch/id_ecdsa.pub authorized_keys
    umask 377
    cp /safecoin-scratch/id_ecdsa id_ecdsa
    echo \"
      Host *
      BatchMode yes
      IdentityFile ~/.ssh/id_ecdsa
      StrictHostKeyChecking no
    \" > config
  "
fi
