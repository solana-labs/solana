#!/bin/bash

command=$1
ip_addr_file=
remote_user=
ssh_keys=

shift

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 <start|stop> <-f IP Addr Array file> <-u username> [-k ssh-keys]

Manage a GCE multinode network

 start|stop    - Create or delete the network
 -f file       - A bash script that exports an array of IP addresses, ip_addr_array.
                 Elements of the array are public IP address of remote nodes.
 -u username   - The username for logging into remote nodes.
 -k ssh-keys   - Path to public/private key pair that remote nodes can use to perform
                 rsync and ssh among themselves. Must contain pub, and priv keys.

EOF
  exit $exitcode
}

while getopts "h?f:u:k:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  f)
    ip_addr_file=$OPTARG
    ;;
  u)
    remote_user=$OPTARG
    ;;
  k)
    ssh_keys=$OPTARG
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

set -e

# Sample IP Address array file contents
# ip_addr_array=(192.168.1.1 192.168.1.5 192.168.2.2)

[[ -n $command ]] || usage "Need a command (start|stop)"
[[ -n $ip_addr_file ]] || usage "Need a file with IP address array"
[[ -n $remote_user ]] || usage "Need the username for remote nodes"

ip_addr_array=()
# Get IP address array
# shellcheck source=/dev/null
source "$ip_addr_file"

build_project() {
  echo "Build started at $(date)"
  SECONDS=0

  # Build and install locally
  PATH="$HOME"/.cargo/bin:"$PATH"
  cargo install --force

  echo "Build took $SECONDS seconds"
}

common_start_setup() {
  ip_addr=$1

  # Killing sshguard for now. TODO: Find a better solution
  # sshguard is blacklisting IP address after ssh-keyscan and ssh login attempts
  ssh "$remote_user@$ip_addr" " \
    set -ex; \
    sudo service sshguard stop; \
    sudo apt-get --assume-yes install rsync libssl-dev; \
    mkdir -p ~/.ssh ~/solana ~/.cargo/bin; \
  " >log/"$ip_addr".log

  # If provided, deploy SSH keys
  if [[ -n $ssh_keys ]]; then
    {
      rsync -vPrz "$ssh_keys"/id_rsa "$remote_user@$ip_addr":~/.ssh/
      rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/
      rsync -vPrz "$ssh_keys"/id_rsa.pub "$remote_user@$ip_addr":~/.ssh/authorized_keys
      rsync -vPrz ./multinode-demo "$remote_user@$ip_addr":~/solana/
    } >>log/"$ip_addr".log
  fi
}

start_leader() {
  common_start_setup "$1"

  {
    rsync -vPrz ~/.cargo/bin/solana* "$remote_user@$ip_addr":~/.cargo/bin/
    rsync -vPrz ./fetch-perf-libs.sh "$remote_user@$ip_addr":~/solana/
    ssh -n -f "$remote_user@$ip_addr" 'cd solana; FORCE=1 ./multinode-demo/remote_leader.sh'
  } >>log/"$1".log

  leader_ip=$1
  leader_time=$SECONDS
  SECONDS=0
}

start_validator() {
  common_start_setup "$1"

  ssh -n -f "$remote_user@$ip_addr" "cd solana; FORCE=1 ./multinode-demo/remote_validator.sh $leader_ip" >>log/"$1".log
}

start_all_nodes() {
  echo "Deployment started at $(date)"
  SECONDS=0
  count=0
  leader_ip=
  leader_time=

  mkdir -p log

  for ip_addr in "${ip_addr_array[@]}"; do
    if ((!count)); then
      # Start the leader on the first node
      echo "Leader node $ip_addr, killing previous instance and restarting"
      start_leader "$ip_addr"
    else
      # Start validator on all other nodes
      echo "Validator[$count] node $ip_addr, killing previous instance and restarting"
      start_validator "$ip_addr" &
      # TBD: Remove the sleep or reduce time once GCP login quota is increased
      sleep 2
    fi

    ((count = count + 1))
  done

  wait

  ((validator_count = count - 1))

  echo "Deployment finished at $(date)"
  echo "Leader deployment too $leader_time seconds"
  echo "$validator_count Validator deployment took $SECONDS seconds"
}

stop_all_nodes() {
  SECONDS=0
  local count=0
  for ip_addr in "${ip_addr_array[@]}"; do
    ssh-keygen -R "$ip_addr" >log/local.log
    ssh-keyscan "$ip_addr" >>~/.ssh/known_hosts 2>/dev/null

    echo "Stopping node[$count] $ip_addr. Remote user $remote_user"

    ssh -n -f "$remote_user@$ip_addr" " \
    set -ex; \
    sudo service sshguard stop; \
    pkill -9 solana-; \
    pkill -9 validator; \
    pkill -9 leader; \
    "
    sleep 2
    ((count = count + 1))
    echo "Stopped node[$count] $ip_addr"
  done
  echo "Stopping $count nodes took $SECONDS seconds"
}

if [[ $command == "start" ]]; then
  build_project
  stop_all_nodes
  start_all_nodes
elif [[ $command == "stop" ]]; then
  stop_all_nodes
else
  usage "Unknown command: $command"
fi
