#!/bin/bash
export leader=10.0.5.135
export ips="54.209.198.32 35.174.139.220 54.89.72.66 54.234.242.128 52.23.253.49 52.90.134.252 18.206.88.129 34.238.38.197"
hash=$(git rev-parse HEAD)

for ff in $ips; do
    ssh-keyscan $ff >> ~/.ssh/known_hosts
    ssh $ff -n "ssh-keyscan $leader >> ~/.ssh/known_hosts"
    ssh $ff -n "cd /home/ubuntu/solana && rsync -v -e ssh $leader:~/solana/genesis.log ."
    ssh $ff -n "cd /home/ubuntu/solana && git fetch origin"
    ssh $ff -n "cd /home/ubuntu/solana && git reset --hard $hash"
    ssh $ff -n "export PATH=/home/ubuntu/.cargo:$PATH && cd /home/ubuntu/solana && cargo run --bin solana-fullnode-config -- -d -b 9000 > validator.json"
done

