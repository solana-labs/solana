#!/bin/bash

command=$1
prefix=
num_nodes=
out_file=
image_name="ubuntu-16-04-cuda-9-2-new"

shift

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 <create|delete> <-p prefix> <-n num_nodes> <-o file> [-i image-name]

Manage a GCE multinode network

 create|delete    - Create or delete the network
 -p prefix        - A common prefix for node names, to avoid collision
 -n num_nodes     - Number of nodes
 -o out_file      - Used for create option. Outputs an array of IP addresses
                    of new nodes to the file
 -i image_name    - Existing image on GCE (default $image_name)

EOF
  exit $exitcode
}

while getopts "h?p:i:n:o:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  p)
    prefix=$OPTARG
    ;;
  i)
    image_name=$OPTARG
    ;;
  o)
    out_file=$OPTARG
    ;;
  n)
    num_nodes=$OPTARG
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

set -e

[[ -n $command ]] || usage "Need a command (create|delete)"

[[ -n $prefix ]] || usage "Need a prefix for GCE instance names"

[[ -n $num_nodes ]] || usage "Need number of nodes"

nodes=()
for i in $(seq 1 "$num_nodes"); do
  nodes+=("$prefix$i")
done

if [[ $command == "create" ]]; then
  [[ -n $out_file ]] || usage "Need an outfile to store IP Addresses"

  ip_addr_list=$(gcloud beta compute instances create "${nodes[@]}" --zone=us-west1-b --tags=testnet \
    --image="$image_name" | awk '/RUNNING/ {print $5}')

  echo "ip_addr_array=($ip_addr_list)" >"$out_file"
elif [[ $command == "delete" ]]; then
  gcloud beta compute instances delete "${nodes[@]}"
else
  usage "Unknown command: $command"
fi
