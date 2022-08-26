#!/usr/bin/env bash
#
# Start a a node
#

here=$(dirname "$0")
source "$here"/common.sh

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage for gossip-sim: $0

Start a validator node with default stake

EOF
  exit 1
}

writeKeys=false
positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    echo "input: $1"
    # validator.sh-only options
    if [[ $1 = --account-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --bootstrap ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --num-nodes ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --entrypoint ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-host ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --write-keys ]]; then
      writeKeys=true
      args+=("$1")
      shift
    elif [[ $1 = --num-keys ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = -h ]]; then
      usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

if [[ $writeKeys == "true" ]]; then
  echo "writeKeys true"
  gossip-sim "${args[@]}"
else
  echo "writeKeys false"
  gossip-sim "${args[@]}" &
  pid=$!
  echo "pid: $pid"
fi
