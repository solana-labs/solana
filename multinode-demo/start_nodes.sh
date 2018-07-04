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

build_time=$SECONDS
echo "Build took $SECONDS seconds"

ip_addr_array=()
# Get IP address array
# shellcheck source=/dev/null
source "$ip_addr_file"

echo "Deployment started at $(date)"
SECONDS=0
count=0
leader_ip=
leader_time=

mkdir -p log

common_setup() {
  ip_addr=$1

  # Killing sshguard for now. TODO: Find a better solution
  # sshguard is blacklisting IP address after ssh-keyscan and ssh login attempts
  ssh -n -f "$remote_user@$ip_addr" "
    set -ex; \
    sudo service sshguard stop; \
    sudo apt-get --assume-yes install rsync libssl-dev; \
    pkill -9 solana-; \
    pkill -9 validator; \
    pkill -9 leader; \
  " >log/"$ip_addr".log

  # If provided, deploy SSH keys
  if [[ -n $ssh_keys ]]; then
    {
      rsync -vPrz "$ssh_keys"/id_rsa "$remote_user@$ip_addr":~/.ssh/
      rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/
      rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/authorized_keys
    } >>log/"$ip_addr".log
  fi
}

leader() {
  common_setup "$1"

  {
    rsync -vPrz ~/.cargo/bin/solana* "$remote_user@$ip_addr":~/.cargo/bin/
    rsync -vPrz ./multinode-demo "$remote_user@$ip_addr":~/solana/
    rsync -vPrz ./fetch-perf-libs.sh "$remote_user@$ip_addr":~/solana/
    ssh -n -f "$remote_user@$ip_addr" 'cd solana; FORCE=1 ./multinode-demo/remote_leader.sh'
  } >>log/"$1".log

  leader_ip=$1
  leader_time=$SECONDS
  SECONDS=0
}

validator() {
  common_setup "$1"

  ssh "$remote_user@$ip_addr" "rsync -vPrz ""$remote_user@$leader_ip"":~/solana/multinode-demo ~/solana/" >>log/"$1".log
  ssh -n -f "$remote_user@$ip_addr" "cd solana; FORCE=1 ./multinode-demo/remote_validator.sh $leader_ip" >>log/"$1".log
}

for ip_addr in "${ip_addr_array[@]}"; do
  ssh-keygen -R "$ip_addr" >log/local.log
  ssh-keyscan "$ip_addr" >>~/.ssh/known_hosts 2>/dev/null

  if ((!count)); then
    # Start the leader on the first node
    echo "Leader node $ip_addr, killing previous instance and restarting"
    leader "$ip_addr"
  else
    # Start validator on all other nodes
    echo "Validator[$count] node $ip_addr, killing previous instance and restarting"
    validator "$ip_addr" &
    # TBD: Remove the sleep or reduce time once GCP login quota is increased
    sleep 2
  fi

  ((count++))
done

wait

((validator_count = count - 1))

echo "Deployment finished at $(date)"
echo "Build took $build_time seconds"
echo "Leader deployment too $leader_time seconds"
echo "$validator_count Validator deployment took $SECONDS seconds"
