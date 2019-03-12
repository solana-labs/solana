#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/../..

set -x
deployMethod="$1"
nodeType="$2"
publicNetwork="$3"
entrypointIp="$4"
numNodes="$5"
RUST_LOG="$6"
skipSetup="$7"
leaderRotation="$8"
set +x
export RUST_LOG=${RUST_LOG:-solana=warn} # if RUST_LOG is unset, default to warn

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]]  || missing deployMethod
[[ -n $nodeType ]]      || missing nodeType
[[ -n $publicNetwork ]] || missing publicNetwork
[[ -n $entrypointIp ]]  || missing entrypointIp
[[ -n $numNodes ]]      || missing numNodes
[[ -n $skipSetup ]]     || missing skipSetup
[[ -n $leaderRotation ]] || missing leaderRotation

cat > deployConfig <<EOF
deployMethod="$deployMethod"
entrypointIp="$entrypointIp"
numNodes="$numNodes"
leaderRotation=$leaderRotation
EOF

source net/common.sh
loadConfigFile

case $deployMethod in
local|tar)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export RUST_LOG
  export SOLANA_METRICS_DISPLAY_HOSTNAME=1

  # Setup `/var/snap/solana/current` symlink so rsyncing the genesis
  # ledger works (reference: `net/scripts/install-rsync.sh`)
  sudo rm -rf /var/snap/solana/current
  sudo mkdir -p /var/snap/solana
  sudo ln -sT /home/solana/solana /var/snap/solana/current

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh

  (
    sudo scripts/oom-monitor.sh
  ) > oom-monitor.log 2>&1 &
  echo $! > oom-monitor.pid
  scripts/net-stats.sh  > net-stats.log 2>&1 &
  echo $! > net-stats.pid

  case $nodeType in
  bootstrap-leader)
    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi
    set -x
    if [[ $skipSetup != true ]]; then
      ./multinode-demo/setup.sh -t bootstrap-leader
    fi
    ./multinode-demo/drone.sh > drone.log 2>&1 &

    maybeNoLeaderRotation=
    if ! $leaderRotation; then
      maybeNoLeaderRotation="--no-leader-rotation"
    fi
    maybePublicAddress=
    if $publicNetwork; then
      maybePublicAddress="--public-address"
    fi

    ./multinode-demo/bootstrap-leader.sh $maybeNoLeaderRotation $maybePublicAddress > bootstrap-leader.log 2>&1 &
    ln -sTf bootstrap-leader.log fullnode.log
    ;;
  fullnode|blockstreamer)
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/.cargo/bin/ ~/.cargo/bin/

    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi

    args=()
    if ! $leaderRotation; then
      args+=("--no-leader-rotation")
    fi
    if $publicNetwork; then
      args+=("--public-address")
    fi
    if [[ $nodeType = blockstreamer ]]; then
      args+=(
        --blockstream /tmp/solana-blockstream.sock
        --no-signer
      )
    fi

    set -x
    if [[ $skipSetup != true ]]; then
      ./multinode-demo/setup.sh -t fullnode
    fi

    if [[ $nodeType = blockstreamer ]]; then
      npm install @solana/blockexplorer@1
      npx solana-blockexplorer > blockexplorer.log 2>&1 &
      sudo socat TCP-LISTEN:80,reuseaddr,fork,su=nobody TCP:localhost:5000
    fi
    ./multinode-demo/fullnode.sh "${args[@]}" "$entrypointIp":~/solana "$entrypointIp:8001" > fullnode.log 2>&1 &
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
