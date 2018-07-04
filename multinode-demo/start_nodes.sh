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

echo "Build started at $(date)"
SECONDS=0
# Build and install locally
PATH="$HOME"/.cargo/bin:"$PATH"
cargo install --force

echo "Build took $SECONDS seconds"

ip_addr_array=()
# Get IP address array
# shellcheck source=/dev/null
source "$ip_addr_file"

# shellcheck disable=SC2089,SC2016
ssh_command_prefix='export PATH="$HOME/.cargo/bin:$PATH"; cd solana; USE_INSTALL=1'

echo "Deployment started at $(date)"
SECONDS=0
count=0
leader_ip=

common_setup() {
  ip_addr=$1

  ssh -n -f "$remote_user@$ip_addr" 'mkdir -p ~/.ssh ~/solana ~/.cargo/bin'

  # Killing sshguard for now. TODO: Find a better solution
  # sshguard is blacklisting IP address after ssh-keyscan and ssh login attempts
  ssh -n -f "$remote_user@$ip_addr" "sudo service sshguard stop"
  ssh -n -f "$remote_user@$ip_addr" 'sudo apt-get --assume-yes install rsync libssl-dev'

  # If provided, deploy SSH keys
  if [[ -z $ssh_keys ]]; then
    echo "skip copying the ssh keys"
  else
    rsync -vPrz "$ssh_keys"/id_rsa "$remote_user@$ip_addr":~/.ssh/
    rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/
    rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/authorized_keys
    ssh -n -f "$remote_user@$ip_addr" 'chmod 600 ~/.ssh/authorized_keys ~/.ssh/id_rsa'
  fi

  # Stop current nodes
  ssh "$remote_user@$ip_addr" 'pkill -9 solana-'
}

leader() {
  common_setup "$1"

  rsync -vPrz ~/.cargo/bin/solana* "$remote_user@$ip_addr":~/.cargo/bin/ # Deploy build and scripts to remote node
  rsync -vPrz ./multinode-demo "$remote_user@$ip_addr":~/solana/
  rsync -vPrz ./fetch-perf-libs.sh "$remote_user@$ip_addr":~/solana/

  # Run setup
  ssh "$remote_user@$ip_addr" "$ssh_command_prefix"' ./multinode-demo/setup.sh -p "$ip_addr"'

  echo "Starting leader node $ip_addr"
  ssh -n -f "$remote_user@$ip_addr" 'cd solana; ./fetch-perf-libs.sh'
  ssh -n -f "$remote_user@$ip_addr" "$ssh_command_prefix"' SOLANA_CUDA=1 ./multinode-demo/leader.sh > leader.log 2>&1'
  ssh -n -f "$remote_user@$ip_addr" "$ssh_command_prefix"' ./multinode-demo/drone.sh > drone.log 2>&1'
  leader_ip=${ip_addr_array[0]}
}

validator() {
  common_setup "$1"

  echo "Adding known hosts for $ip_addr"
  ssh "$remote_user@$ip_addr" "ssh-keygen -R ""$leader_ip"
  ssh "$remote_user@$ip_addr" "ssh-keyscan ""$leader_ip >> ~/.ssh/known_hosts"

  ssh "$remote_user@$ip_addr" "rsync -vPrz ""$remote_user@$leader_ip"":~/.cargo/bin/solana* ~/.cargo/bin/"
  ssh "$remote_user@$ip_addr" "rsync -vPrz ""$remote_user@$leader_ip"":~/solana/multinode-demo ~/solana/"
  ssh "$remote_user@$ip_addr" "rsync -vPrz ""$remote_user@$leader_ip"":~/solana/fetch-perf-libs.sh ~/solana/"

  # Run setup
  ssh "$remote_user@$ip_addr" "$ssh_command_prefix"' ./multinode-demo/setup.sh -p "$ip_addr"'

  echo "Starting validator node $ip_addr"
  ssh -n -f "$remote_user@$ip_addr" "$ssh_command_prefix"" ./multinode-demo/validator.sh $remote_user@$leader_ip:~/solana $leader_ip > validator.log 2>&1"
}

for ip_addr in "${ip_addr_array[@]}"; do
  echo "$ip_addr"
  ssh-keygen -R "$ip_addr"
  ssh-keyscan "$ip_addr" >>~/.ssh/known_hosts

  if ((!count)); then
    # Start the leader on the first node
    leader "$ip_addr"
  else
    # Start validator on all other nodes
    validator "$ip_addr" &
    # TBD: Remove the sleep or reduce time once GCP login quota is increased
    sleep 2
  fi

  ((count++))
done

wait

echo "Deployment finished at $(date)"
echo "Deployment took $SECONDS seconds"
