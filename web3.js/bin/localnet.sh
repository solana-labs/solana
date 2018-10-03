#!/bin/bash -e

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [update|up|down|logs] [command-specific options]

Operate a local testnet

 update   - Update the network image from dockerhub.com
 up       - Start the network
 down     - Stop the network
 logs     - Display network logging


 logs-specific options:
   -f     - Follow log output

 update/up-specific options:
   edge   - Update/start the "edge" channel network
   beta   - Update/start the "beta" channel network (default)

 down-specific options:
   none


Assumes that docker is installed
EOF
  exit $exitcode
}

[[ -n $1 ]] || usage
cmd="$1"

channel=beta

docker --version || usage "It appears that docker is not installed"
case $cmd in
update)
  if [[ -n $2 ]]; then
    channel="$2"
  fi
  [[ $channel = edge || $channel = beta ]] || usage "Invalid channel: $channel"

  (
    set -x
    docker pull solanalabs/solana:"$channel"
  )
  ;;
up)
  if [[ -n $2 ]]; then
    channel="$2"
  fi
  [[ $channel = edge || $channel = beta ]] || usage "Invalid channel: $channel"

  (
    set -x
    docker run \
      --detach \
      --name solana-localnet \
      --rm \
      --publish 8899:8899 \
      --tty \
      solanalabs/solana:"$channel"

  )
  ;;
down)
  (
    set -x
    docker stop --time 0 solana-localnet
  )
  ;;
logs)
  (
    set -x
    docker logs solana-localnet "$@"
  )
  ;;
*)
  usage "Unknown command: $cmd"
esac

exit 0
