#!/bin/bash

here=$(dirname "$0")
# shellcheck source=scripts/gcloud.sh
source "$here"/../scripts/gcloud.sh

command=$1
prefix=
num_nodes=
out_file=
image_name="ubuntu-16-04-cuda-9-2-new"
internalNetwork=false
zone="us-west1-b"

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
 -P               - Use IP addresses on GCE internal/private network
 -z               - GCP Zone for the nodes (default $zone)
 -o out_file      - Used for create option. Outputs an array of IP addresses
                    of new nodes to the file
 -i image_name    - Existing image on GCE (default $image_name)

EOF
  exit $exitcode
}

while getopts "h?p:Pi:n:z:o:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  p)
    prefix=$OPTARG
    ;;
  P)
    internalNetwork=true
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
  z)
    zone=$OPTARG
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

set -e

[[ -n $command ]] || usage "Need a command (create|delete)"

[[ -n $prefix ]] || usage "Need a prefix for GCE instance names"


if [[ $command == "create" ]]; then
  [[ -n $num_nodes ]] || usage "Need number of nodes"
  [[ -n $out_file ]] || usage "Need an outfile to store IP Addresses"

  gcloud_CreateInstances "$prefix" "$num_nodes" "$zone" "$image_name"
  gcloud_FindInstances "name~^$prefix"

  echo "ip_addr_array=()" > "$out_file"
  recordPublicIp() {
    declare name="$1"
    declare publicIp="$3"
    declare privateIp="$4"

    if $internalNetwork; then
      echo "ip_addr_array+=($privateIp) # $name" >> "$out_file"
    else
      echo "ip_addr_array+=($publicIp)  # $name" >> "$out_file"
    fi
  }
  gcloud_ForEachInstance recordPublicIp

  echo "Instance ip addresses recorded in $out_file"
elif [[ $command == "delete" ]]; then
  gcloud_FindInstances "name~^$prefix"

  if [[ ${#instances[@]} -eq 0 ]]; then
    echo "No instances found matching '^$prefix'"
    exit 0
  fi
  gcloud_DeleteInstances
else
  usage "Unknown command: $command"
fi
