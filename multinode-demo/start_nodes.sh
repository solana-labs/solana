#!/bin/bash

nodes_file=$1
remote_user=$2
ssh_keys=$3

oldIFS="$IFS"
IFS=$'\n' ip_addr_array=($(<$nodes_file))
IFS="$oldIFS"

ssh_command_prefix='export PATH="$HOME/.cargo/bin:$PATH"; cd solana; USE_INSTALL=1 ./multinode-demo/'

count=0
leader=
for ip_addr in "${ip_addr_array[@]}"; do
  echo "$ip_addr"

  rsync -r -av ~/.cargo/bin "$remote_user"@"$ip_addr":~/.cargo
  rsync -r -av ./multinode-demo "$remote_user"@"$ip_addr":~/solana/
  
  if [[ -z $ssh_keys ]]; then
    echo "skip copying the ssh keys"
  else
    echo "rsync -r -av $ssh_keys/* $remote_user@$ip_addr:~/.ssh/"
  fi

  ssh "$remote_user"@"$ip_addr" $ssh_command_prefix'setup.sh'
  
  if [[ "$count" -eq 0 ]]; then
    # Start the leader on the first node
    echo "Starting leader node $ip_addr"
    ssh -n -f "$remote_user"@"$ip_addr" $ssh_command_prefix'leader.sh > leader.log 2>&1'
    leader=${ip_addr_array[0]}
  else
    # Start validator on all other nodes
    echo "Starting validator node $ip_addr"
    ssh -n -f "$remote_user"@"$ip_addr" $ssh_command_prefix"validator.sh $remote_user@$leader:~/solana $leader > validator.log 2>&1"
  fi

  (( count++ ))

  if [[ "$count" -eq ${#ip_addr_array[@]} ]]; then
    # Launch client demo on the last node
    echo "Starting client demo on $ip_addr"
    ssh -n -f "$remote_user"@"$ip_addr" $ssh_command_prefix"client.sh $remote_user@$leader:~/solana $count > client.log 2>&1"
  fi
done

