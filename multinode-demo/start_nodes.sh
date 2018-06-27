#!/bin/bash

# Format for the nodes file
# node1 public-ip|node1 privte-ip
# node2 public-ip|node2 privte-ip
# ....
# e.g.
# 35.197.92.75|10.138.0.6
# 35.185.232.222|10.138.0.3
# 35.233.137.53|10.138.0.5

nodes_file=$1
remote_user=$2
ssh_keys=$3

declare -A ip_addr_array
while IFS=\| read public private
do
  ip_addr_array[$public]=$private
done < $nodes_file

count=0
leader=
for public_ip in ${!ip_addr_array[*]}
do
  rsync -r -av ~/.cargo/bin $remote_user@$public_ip:~/.cargo
  rsync -r -av ./multinode-demo $remote_user@$public_ip:~/solana/
  
  if [ -z $ssh_keys ]
  then
    echo "skip copying the ssk keys"
  else
    echo "rsync -r -av $ssh_keys/* $remote_user@$public_ip:~/.ssh/"
  fi

  ssh $remote_user@$public_ip 'source .cargo/env; cd solana; USE_INSTALL=1 ./multinode-demo/setup.sh'
  
  if [ $count -eq 0 ]
  then
    # Start the leader on the first node
    echo "Starting leader node $public_ip"
    ssh -n -f $remote_user@$public_ip 'source .cargo/env; cd solana; USE_INSTALL=1 ./multinode-demo/leader.sh > leader.log 2>&1'
    leader=${ip_addr_array[$public_ip]}
  else
    # Start validator on all other nodes
    echo "Starting validator node $public_ip"
    ssh -n -f $remote_user@$public_ip "source .cargo/env; cd solana; USE_INSTALL=1 ./multinode-demo/validator.sh $remote_user@$leader:~/solana $leader > validator.log 2>&1"
  fi

  (( count++ ))

  if [ $count -eq ${#ip_addr_array[@]} ]
  then
    # Launch client demo on the last node
    echo "Starting client demo on $public_ip"
    ssh -n -f $remote_user@$public_ip "source .cargo/env; cd solana; USE_INSTALL=1 ./multinode-demo/client.sh $remote_user@$leader:~/solana $count > client.log 2>&1"
  fi

done

