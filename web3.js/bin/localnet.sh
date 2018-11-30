#!/usr/bin/env bash
set -e

channel=$(cd "$(dirname "$0")"; node -p 'require("../package.json")["solana-channel"]')

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [update|up|down|logs|deploy] [command-specific options]

Operate a local testnet

 update   - Update the network image from dockerhub.com
 up       - Start the network
 down     - Stop the network
 logs     - Display network logging
 deploy   - Deploy a native program.


 logs-specific options:
   -f     - Follow log output

 update/up-specific options:
   edge   - Update/start the "edge" channel network
   beta   - Update/start the "beta" channel network

   Default channel: $channel

 down-specific options:
   none

 deploy-specific options:
   program - The program to deploy.

   Note that deployments are discarded on network stop

EOF
  exit $exitcode
}

[[ -n $1 ]] || usage
cmd="$1"
shift

docker --version || usage "It appears that docker is not installed"
case $cmd in
update)
  if [[ -n $1 ]]; then
    channel="$1"
  fi
  [[ $channel = edge || $channel = beta ]] || usage "Invalid channel: $channel"

  (
    set -x
    docker pull solanalabs/solana:"$channel"
  )
  ;;
up)
  if [[ -n $1 ]]; then
    channel="$1"
  fi
  [[ $channel = edge || $channel = beta ]] || usage "Invalid channel: $channel"

  (
    set -x
    RUST_LOG=${RUST_LOG:-solana=warn,solana_bpf=info,solana_jsonrpc=info,solana::rpc=info,solana_fullnode=info,solana::drone=info,solana::bank=info,solana::banking_stage=info,solana::system_program=info}

    docker run \
      --detach \
      --name solana-localnet \
      --rm \
      --publish 8899:8899 \
      --publish 8900:8900 \
      --tty \
      --env RUST_LOG="$RUST_LOG" \
      solanalabs/solana:"$channel"

    for _ in 1 2 3 4 5; do
      if curl \
          -X POST \
          -H "Content-Type: application/json" \
          -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
          http://localhost:8899; then
        break;
      fi
      sleep 1
    done
  )
  ;;
down)
  (
    set -x
    docker stop --time 0 solana-localnet
  )
  ;;
logs)
  follow=false
  if [[ -n $1 ]]; then
    if [[ $1 = "-f" ]]; then
      follow=true
    else
      usage "Unknown argument: $1"
    fi
  fi

  while $follow; do
    if [[ -n $(docker ps -q -f name=solana-localnet) ]]; then
      (
        set -x
        docker logs solana-localnet -f
      ) || true
    fi
    sleep 1
  done

  (
    set -x
    docker logs solana-localnet
  )
  ;;
deploy)
  program=$1
  [[ -n $program ]] || usage
  [[ -f $program ]] || usage "file does not exist: $program"

  basename=$(basename "$program")
  if docker exec solana-localnet test -f /usr/bin/"$basename"; then
    echo "Error: $basename has already been deployed"
    exit 1
  fi

  (
    set -x
    docker cp "$program" solana-localnet:/usr/bin/
  )
  docker exec solana-localnet ls -l /usr/bin/"$basename"
  echo "$basename deployed successfully"
  ;;
*)
  usage "Unknown command: $cmd"
esac

exit 0
