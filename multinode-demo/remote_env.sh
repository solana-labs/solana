#!/bin/bash

usage() {
  echo -e "\\tUsage: source this-script <IP Address array> <username> [path to ssh keys]\\n"
  echo -e "\\t <IP Address array>: A bash script that exports an array of IP addresses, SOLANA_NODE_IP_ADDRS. Elements of the array are public IP address of remote nodes."
  echo -e "\\t <username>:         The username for logging into remote nodes."
  echo -e "\\t [path to ssh keys]: The public/private key pair that remote nodes can use to perform rsync and ssh among themselves. Must contain pub, priv and authorized_keys.\\n"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo -e "\\n\\tLooks like you are trying to run ${0}"
  echo -e "\\tThe script must be sourced in your bash environment.\\n"
  usage
  exit 1
fi

ip_addr_file=$1
export SOLANA_NODE_REMOTE_USER=$2
export SOLANA_NODE_SSH_KEYS=$3

# Sample IP Address array file contents
# SOLANA_NODE_IP_ADDRS=(192.168.1.1 192.168.1.5 192.168.2.2)

if [[ -z "$ip_addr_file" ]]; then
  usage
  return
fi

if [[ -z "$SOLANA_NODE_REMOTE_USER" ]]; then
  usage
  return
fi

SOLANA_NODE_IP_ADDRS=()
# Get IP address array
# shellcheck source=/dev/null
source "$ip_addr_file"

build_solana_nodes() {
  # Build and install locally
  PATH="$HOME"/.cargo/bin:"$PATH"
  cargo install --force
}

start_solana_nodes() {
  # shellcheck disable=SC2089,SC2016
  ssh_command_prefix='export PATH="$HOME/.cargo/bin:$PATH"; cd solana; USE_INSTALL=1 ./multinode-demo/'

  count=0
  leader=
  for ip_addr in "${SOLANA_NODE_IP_ADDRS[@]}"; do
    # Deploy build and scripts to remote node
    rsync -r -av ~/.cargo/bin "$SOLANA_NODE_REMOTE_USER"@"$ip_addr":~/.cargo
    rsync -r -av ./multinode-demo "$SOLANA_NODE_REMOTE_USER"@"$ip_addr":~/solana/

    # If provided, deploy SSH keys
    if [[ -z $SOLANA_NODE_SSH_KEYS ]]; then
      echo "skip copying the ssh keys"
    else
      rsync -r -av "$SOLANA_NODE_SSH_KEYS"/* "$SOLANA_NODE_REMOTE_USER"@"$ip_addr":~/.ssh/
    fi

    # Stop current nodes
    ssh "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" 'pkill -9 solana-fullnode'
    ssh "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" 'pkill -9 solana-client-demo'

    # Run setup
    ssh "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" "$ssh_command_prefix"'setup.sh -p "$ip_addr"'

    if ((!count)); then
      # Start the leader on the first node
      echo "Starting leader node $ip_addr"
      ssh -n -f "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" "$ssh_command_prefix"'leader.sh > leader.log 2>&1'
      leader=${SOLANA_NODE_IP_ADDRS[0]}
    else
      # Start validator on all other nodes
      echo "Starting validator node $ip_addr"
      ssh -n -f "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" "$ssh_command_prefix""validator.sh $SOLANA_NODE_REMOTE_USER@$leader:~/solana $leader > validator.log 2>&1"
    fi

    ((count++))
  done
}

stop_solana_nodes() {
  for ip_addr in "${SOLANA_NODE_IP_ADDRS[@]}"; do
    echo "Stopping node $ip_addr"

    # Stop current nodes
    ssh "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" 'pkill -9 solana-fullnode'
    ssh "$SOLANA_NODE_REMOTE_USER"@"$ip_addr" 'pkill -9 solana-client-demo'
  done
}

echo -e "\\tFollowing bash functions are available in your shell\\n"
echo -e "\\t build_solana_nodes # To build code that'll be deployed to remote nodes"
echo -e "\\t start_solana_nodes # To launch remote nodes"
echo -e "\\t stop_solana_nodes  # To stop remote nodes\\n"
