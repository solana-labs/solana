#!/usr/bin/env bash
#
# Reconfigure netem on remote nodes
#
set -e

usage() {
  echo "Usage: $0 <% of network> <netem configuration>"
  echo
  echo "% of network         - Percentage of network that should be reconfigured"
  echo "neten configuration  - Duble quoted string with new netem configuration"
  echo
}

if [ "$#" -lt 2 ]; then
  usage
  exit 1
fi

cd "$(dirname "$0")"

source ../net/config/config

num_nodes=$((${#validatorIpList[@]}*$1/100))
if [[ $((${#validatorIpList[@]}*$1%100)) -gt 0 ]]; then
  num_nodes=$((num_nodes+1))
fi
if [[ "$num_nodes" -gt "${#validatorIpList[@]}" ]]; then
  num_nodes=${#validatorIpList[@]}
fi

# Stop netem on all nodes
for i in "${!validatorIpList[@]}"; do :
  ../net/ssh.sh solana@${validatorIpList[$i]} '~/solana/scripts/netem.sh delete < ~/solana/netem.cfg || true'
done

# Start netem on required nodes
for ((i=0; i<$num_nodes; i++ )); do :
  ../net/ssh.sh solana@${validatorIpList[$i]} "echo $2 > ~/solana/netem.cfg; ~/solana/scripts/netem.sh add \"$2\""
done