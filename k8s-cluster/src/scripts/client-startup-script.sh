#!/bin/bash

clientToRun="$1"
benchTpsExtraArgs="$2"
clientType=

# check if benchTpsExtraArgs is set. if not then it will get set to client-type. Which then needs to get handled appropriately
if [[ "$benchTpsExtraArgs" == "thin-client" || "$benchTpsExtraArgs" == "tpu-client" || "$benchTpsExtraArgs" == "rpc-client" ]]; then
    clientType=$benchTpsExtraArgs
    benchTpsExtraArgs=
    shift 2
else
    clientType="${3:-thin-client}"
    shift 3
    # Convert string to array
    IFS=' ' read -r -a argsArray <<< "$benchTpsExtraArgs"

    # Initialize clientType with a default value
    clientType="thin-client"

    # Loop through the array and check for the specific flag
    for arg in "${argsArray[@]}"; do
        if [ "$arg" == "--use-rpc-client" ]; then
            clientType="rpc-client"
            break
        elif [ "$arg" == "--use-tpu-client" ]; then
            clientType="tpu-client"
            break
        fi
    done
fi


missing() {
  echo "Error: $1 not specified"
  exit 1
}

threadCount=$(nproc)
if [[ $threadCount -gt 4 ]]; then
  threadCount=4
fi

echo "threadCount: $threadCount"

TPU_CLIENT=false
RPC_CLIENT=false
case "$clientType" in
  thin-client)
    TPU_CLIENT=false
    RPC_CLIENT=false
    ;;
  tpu-client)
    TPU_CLIENT=true
    RPC_CLIENT=false
    ;;
  rpc-client)
    TPU_CLIENT=false
    RPC_CLIENT=true
    ;;
  *)
    echo "Unexpected clientType: \"$clientType\""
    exit 1
    ;;
esac

case $clientToRun in
bench-tps)
  args=()

  if ${TPU_CLIENT}; then
    args+=(--url "http://$LOAD_BALANCER_RPC_ADDRESS")
  elif ${RPC_CLIENT}; then
    args+=(--url "http://$LOAD_BALANCER_RPC_ADDRESS")
  else
    args+=(--entrypoint "$BOOTSTRAP_GOSSIP_ADDRESS")
  fi

  clientCommand="\
    solana-bench-tps \
      --sustained \
      --threads $threadCount \
      $benchTpsExtraArgs \
      --read-client-keys ./client-accounts.yml \
      ${args[*]} \
      ${runtime_args[*]} \
  "
  ;;
idle)
  # In net/remote/remote-client.sh, we add faucet keypair here
  # but in this case we already do that in the docker container
  # by default
  while true; do sleep 3600; done
  ;;
*)
  echo "Unknown client name: $clientToRun"
  exit 1
esac

echo "client command to run: $clientCommand"
$clientCommand
