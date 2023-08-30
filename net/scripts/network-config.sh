#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

sudo sysctl -w net.core.rmem_default=134217728
sudo sysctl -w net.core.rmem_max=134217728

sudo sysctl -w net.core.wmem_default=134217728
sudo sysctl -w net.core.wmem_max=134217728

# Increase memory mapped files limit
sudo sysctl -w vm.max_map_count=1000000

# Increase number of allowed open file descriptors
echo "* - nofile 1000000" | sudo tee -a /etc/security/limits.conf

echo "MaxAuthTries 60" | sudo tee -a /etc/ssh/sshd_config
sudo service sshd restart
sudo systemctl restart sshd
