#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/../..

set -x
deployMethod="$1"
nodeType="$2"
entrypointIp="$3"
numNodes="$4"
RUST_LOG="$5"
skipSetup="$6"
failOnValidatorBootupFailure="$7"
externalPrimordialAccountsFile="$8"
stakeNodesInGenesisBlock="$9"
nodeIndex="${10}"
numBenchTpsClients="${11}"
benchTpsExtraArgs="${12}"
numBenchExchangeClients="${13}"
benchExchangeExtraArgs="${14}"
genesisOptions="${15}"
set +x
export RUST_LOG

# Use a very large stake (relative to the default multinode-demo/ stake of 42)
# for the testnet validators setup by net/.  This make it less likely that
# low-staked ephemeral validator a random user may attach to testnet will cause
# trouble
#
# Ref: https://github.com/solana-labs/solana/issues/3798
stake=424243

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]]  || missing deployMethod
[[ -n $nodeType ]]      || missing nodeType
[[ -n $entrypointIp ]]  || missing entrypointIp
[[ -n $numNodes ]]      || missing numNodes
[[ -n $skipSetup ]]     || missing skipSetup
[[ -n $failOnValidatorBootupFailure ]] || missing failOnValidatorBootupFailure

airdropsEnabled=true
if [[ -n $stakeNodesInGenesisBlock ]]; then
  airdropsEnabled=false
fi
cat > deployConfig <<EOF
deployMethod="$deployMethod"
entrypointIp="$entrypointIp"
numNodes="$numNodes"
failOnValidatorBootupFailure=$failOnValidatorBootupFailure
genesisOptions="$genesisOptions"
airdropsEnabled=$airdropsEnabled
EOF

source net/common.sh
loadConfigFile

initCompleteFile=init-complete-node.log
waitForNodeToInit() {
  echo "--- waiting for node to boot up"
  SECONDS=
  while [[ ! -r $initCompleteFile ]]; do
    if [[ $SECONDS -ge 720 ]]; then
      echo "^^^ +++"
      echo "Error: $initCompleteFile not found in $SECONDS seconds"
      exit 1
    fi
    echo "Waiting for $initCompleteFile ($SECONDS)..."
    sleep 5
  done
  echo "Node booted up"
}

case $deployMethod in
local|tar)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export SOLANA_METRICS_DISPLAY_HOSTNAME=1

  # Setup `/var/snap/solana/current` symlink so rsyncing the genesis
  # ledger works (reference: `net/scripts/install-rsync.sh`)
  sudo rm -rf /var/snap/solana/current
  sudo mkdir -p /var/snap/solana
  sudo ln -sT /home/solana/solana /var/snap/solana/current

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh
  SUDO_OK=1 source scripts/tune-system.sh

  (
    sudo SOLANA_METRICS_CONFIG="$SOLANA_METRICS_CONFIG" scripts/oom-monitor.sh
  ) > oom-monitor.log 2>&1 &
  echo $! > oom-monitor.pid
  scripts/net-stats.sh  > net-stats.log 2>&1 &
  echo $! > net-stats.pid

  case $nodeType in
  bootstrap-leader)
    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-validator-cuda ]]; then
      echo Selecting solana-validator-cuda
      export SOLANA_CUDA=1
    fi
    set -x
    rm -rf ./solana-node-keys
    rm -rf ./solana-node-stakes
    mkdir ./solana-node-stakes
    if [[ -n $stakeNodesInGenesisBlock ]]; then
      for i in $(seq 0 "$numNodes"); do
        solana-keygen new -o ./solana-node-keys/"$i"
        pubkey="$(solana-keygen pubkey ./solana-node-keys/"$i")"
        echo "${pubkey}: $stakeNodesInGenesisBlock" >> ./solana-node-stakes/fullnode-stakes.yml
      done
    fi

    lamports_per_signature="42"
    # shellcheck disable=SC2206 # Do not want to quote $genesisOptions
    genesis_args=($genesisOptions)
    for i in "${!genesis_args[@]}"; do
      if [[ "${genesis_args[$i]}" = --target-lamports-per-signature ]]; then
        lamports_per_signature="${genesis_args[$((i+1))]}"
        break
      fi
    done

    rm -rf ./solana-client-accounts
    mkdir ./solana-client-accounts
    for i in $(seq 0 $((numBenchTpsClients-1))); do
      # shellcheck disable=SC2086 # Do not want to quote $benchTpsExtraArgs
      solana-bench-tps --write-client-keys ./solana-client-accounts/bench-tps"$i".yml \
        --target-lamports-per-signature "$lamports_per_signature" $benchTpsExtraArgs
      # Skip first line, as it contains header
      tail -n +2 -q ./solana-client-accounts/bench-tps"$i".yml >> ./solana-client-accounts/client-accounts.yml
      echo "" >> ./solana-client-accounts/client-accounts.yml
    done
    for i in $(seq 0 $((numBenchExchangeClients-1))); do
      # shellcheck disable=SC2086 # Do not want to quote $benchExchangeExtraArgs
      solana-bench-exchange --batch-size 1000 --fund-amount 20000 \
        --write-client-keys ./solana-client-accounts/bench-exchange"$i".yml $benchExchangeExtraArgs
      tail -n +2 -q ./solana-client-accounts/bench-exchange"$i".yml >> ./solana-client-accounts/client-accounts.yml
      echo "" >> ./solana-client-accounts/client-accounts.yml
    done
    [[ -z $externalPrimordialAccountsFile ]] || cat "$externalPrimordialAccountsFile" >> ./solana-node-stakes/fullnode-stakes.yml
    if [ -f ./solana-node-stakes/fullnode-stakes.yml ]; then
      genesisOptions+=" --primordial-accounts-file ./solana-node-stakes/fullnode-stakes.yml"
    fi
    if [ -f ./solana-client-accounts/client-accounts.yml ]; then
      genesisOptions+=" --primordial-keypairs-file ./solana-client-accounts/client-accounts.yml"
    fi
    if [[ $skipSetup != true ]]; then
      args=(
        --bootstrap-leader-stake-lamports "$stake"
      )
      # shellcheck disable=SC2206 # Do not want to quote $genesisOptions
      args+=($genesisOptions)
      ./multinode-demo/setup.sh "${args[@]}"
    fi
    if [[ -z $stakeNodesInGenesisBlock ]]; then
      ./multinode-demo/drone.sh > drone.log 2>&1 &
    fi
    args=(
      --enable-rpc-exit
      --gossip-port "$entrypointIp":8001
    )

    if [[ -n $stakeNodesInGenesisBlock ]]; then
      args+=(--no-airdrop)
    fi
    args+=(--init-complete-file "$initCompleteFile")
    nohup ./multinode-demo/validator.sh --bootstrap-leader "${args[@]}" > fullnode.log 2>&1 &
    waitForNodeToInit
    ;;
  validator|blockstreamer)
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/.cargo/bin/ ~/.cargo/bin/
    rm -f ~/solana/fullnode-identity.json
    [[ -z $stakeNodesInGenesisBlock ]] || net/scripts/rsync-retry.sh -vPrc \
    "$entrypointIp":~/solana/solana-node-keys/"$nodeIndex" ~/solana/fullnode-identity.json

    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-validator-cuda ]]; then
      echo Selecting solana-validator-cuda
      export SOLANA_CUDA=1
    fi

    args=(
      "$entrypointIp":~/solana "$entrypointIp:8001"
      --gossip-port 8001
      --rpc-port 8899
    )
    if [[ $nodeType = blockstreamer ]]; then
      args+=(
        --blockstream /tmp/solana-blockstream.sock
        --no-voting
        --stake 0
        --generate-snapshots
      )
    else
      args+=(--stake "$stake")
      args+=(--enable-rpc-exit)
    fi

    if [[ -f ~/solana/fullnode-identity.json ]]; then
      args+=(--identity ~/solana/fullnode-identity.json)
    fi

    if [[ -n $stakeNodesInGenesisBlock ]]; then
      args+=(--no-airdrop)
    fi

    set -x
    if [[ $skipSetup != true ]]; then
      ./multinode-demo/clear-config.sh
    fi

    if [[ $nodeType = blockstreamer ]]; then
      # Sneak the mint-keypair.json from the bootstrap leader and run another drone
      # with it on the blockstreamer node.  Typically the blockstreamer node has
      # a static IP/DNS name for hosting the blockexplorer web app, and is
      # a location that somebody would expect to be able to airdrop from
      scp "$entrypointIp":~/solana/config-local/mint-keypair.json config-local/
      if [[ -z $stakeNodesInGenesisBlock ]]; then
        ./multinode-demo/drone.sh > drone.log 2>&1 &
      fi

      # Grab the TLS cert generated by /certbot-restore.sh
      if [[ -f /.cert.pem ]]; then
        sudo install -o $UID -m 400 /.cert.pem /.key.pem .
        ls -l .cert.pem .key.pem
      fi

      export BLOCKEXPLORER_GEOIP_WHITELIST=$PWD/net/config/geoip.yml
      npm install @solana/blockexplorer@1
      npx solana-blockexplorer > blockexplorer.log 2>&1 &

      # Confirm the blockexplorer is accessible
      curl --head --retry 3 --retry-connrefused http://localhost:5000/

      # Redirect port 80 to port 5000
      sudo iptables -A INPUT -p tcp --dport 80 -j ACCEPT
      sudo iptables -A INPUT -p tcp --dport 5000 -j ACCEPT
      sudo iptables -A PREROUTING -t nat -p tcp --dport 80 -j REDIRECT --to-port 5000

      # Confirm the blockexplorer is now globally accessible
      curl --head "$(curl ifconfig.io)"
    fi

    args+=(--init-complete-file "$initCompleteFile")
    nohup ./multinode-demo/validator.sh "${args[@]}" > fullnode.log 2>&1 &
    waitForNodeToInit
    ;;
  replicator)
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/.cargo/bin/ ~/.cargo/bin/

    args=(
      "$entrypointIp":~/solana "$entrypointIp:8001"
    )

    if [[ -n $stakeNodesInGenesisBlock ]]; then
      args+=(--no-airdrop)
    fi

    if [[ $skipSetup != true ]]; then
      ./multinode-demo/clear-config.sh
    fi
    nohup ./multinode-demo/replicator.sh "${args[@]}" > fullnode.log 2>&1 &
    sleep 1
    ;;
  *)
    echo "Error: unknown node type: $nodeType"
    exit 1
    ;;
  esac
  disown
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac
