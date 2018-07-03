#!/bin/bash

ip_addr_file=$1
remote_user=$2
ssh_keys=$3

usage() {
  echo -e "\\tUsage: $0 <IP Address array> <username> [path to ssh keys]\\n"
  echo -e "\\t <IP Address array>: A bash script that exports an array of IP addresses, ip_addr_array. Elements of the array are public IP address of remote nodes."
  echo -e "\\t <username>:         The username for logging into remote nodes."
  echo -e "\\t [path to ssh keys]: The public/private key pair that remote nodes can use to perform rsync and ssh among themselves. Must contain pub, priv and authorized_keys.\\n"
  exit 1
}

# Sample IP Address array file contents
# ip_addr_array=(192.168.1.1 192.168.1.5 192.168.2.2)

if [[ -z "$ip_addr_file" ]]; then
  usage
fi

if [[ -z "$remote_user" ]]; then
  usage
fi

# Build and install locally
PATH="$HOME"/.cargo/bin:"$PATH"
cargo install --force

ip_addr_array=()
# Get IP address array
# shellcheck source=/dev/null
source "$ip_addr_file"

# shellcheck disable=SC2089,SC2016
ssh_command_prefix='export PATH="$HOME/.cargo/bin:$PATH"; cd solana; USE_INSTALL=1 ./multinode-demo/'

count=0
leader=
for ip_addr in "${ip_addr_array[@]}"; do
  echo "$ip_addr"

  ssh-keygen -R "$ip_addr"
  ssh-keyscan "$ip_addr" >>~/.ssh/known_hosts

  ssh "$remote_user@$ip_addr" 'mkdir ~/.ssh'

  # Killing sshguard for now. TODO: Find a better solution
  # sshguard is blacklisting IP address after ssh-keyscan and ssh login attempts
  ssh -n -f "$remote_user@$ip_addr" "sudo service sshguard stop"
  ssh -n -f "$remote_user@$ip_addr" 'sudo apt-get --assume-yes install rsync'

  if [[ -n $leader ]]; then
    echo "Adding known hosts for $ip_addr"
    ssh -n -f "$remote_user@$ip_addr" "ssh-keygen -R $leader"
    ssh -n -f "$remote_user@$ip_addr" "ssh-keyscan $leader >> ~/.ssh/known_hosts"
  fi

  # Deploy build and scripts to remote node
  ssh "$remote_user@$ip_addr" 'mkdir ~/solana'
  rsync -r -av ~/.cargo/bin "$remote_user"@"$ip_addr":~/.cargo
  rsync -r -av ./multinode-demo "$remote_user"@"$ip_addr":~/solana/

  # If provided, deploy SSH keys
  if [[ -z $ssh_keys ]]; then
    echo "skip copying the ssh keys"
  else
    rsync -r -av "$ssh_keys"/id_rsa "$remote_user@$ip_addr":~/.ssh/
    rsync -r -av "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/
    rsync -r -av "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/authorized_keys
    ssh -n -f "$remote_user@$ip_addr" 'chmod 600 ~/.ssh/authorized_keys ~/.ssh/id_rsa'
  fi

  # Stop current nodes
  ssh "$remote_user@$ip_addr" 'pkill -9 solana-fullnode'
  ssh "$remote_user@$ip_addr" 'pkill -9 solana-client-demo'

  ssh "$remote_user@$ip_addr" 'sudo apt-get --assume-yes install libssl-dev'

  # Run setup
  ssh "$remote_user@$ip_addr" "$ssh_command_prefix"'setup.sh -p "$ip_addr"'

  if ((!count)); then
    # Start the leader on the first node
    echo "Starting leader node $ip_addr"
    ssh -n -f "$remote_user@$ip_addr" "$ssh_command_prefix"'leader.sh > leader.log 2>&1'
    leader=${ip_addr_array[0]}
  else
    # Start validator on all other nodes
    echo "Starting validator node $ip_addr"
    ssh -n -f "$remote_user@$ip_addr" "$ssh_command_prefix""validator.sh $remote_user@$leader:~/solana $leader > validator.log 2>&1"
  fi

  ((count++))
done
