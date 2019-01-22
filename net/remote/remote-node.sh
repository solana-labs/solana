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

if [[ $publicNetwork = true ]]; then
  setupArgs="-p"
else
  setupArgs="-l"
fi

case $deployMethod in
snap)
  SECONDS=0

  if [[ $skipSetup = true ]]; then
    for configDir in /var/snap/solana/current/config{,-local}; do
      if [[ ! -d $configDir ]]; then
        echo Error: not a directory: $configDir
        exit 1
      fi
    done
    (
      set -x
      sudo rm -rf /saved-node-config
      sudo mkdir /saved-node-config
      sudo mv /var/snap/solana/current/config{,-local} /saved-node-config
    )
  fi

  [[ $nodeType = bootstrap-leader ]] ||
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/solana/solana.snap" .
  if snap list solana; then
    sudo snap remove solana
  fi
  sudo snap install solana.snap --devmode --dangerous

  if [[ $skipSetup = true ]]; then
    (
      set -x
      sudo rm -rf /var/snap/solana/current/config{,-local}
      sudo mv /saved-node-config/* /var/snap/solana/current/
      sudo rm -rf /saved-node-config
    )
  fi

  # shellcheck disable=SC2089
  commonNodeConfig="\
    entrypoint-ip=\"$entrypointIp\" \
    metrics-config=\"$SOLANA_METRICS_CONFIG\" \
    rust-log=\"$RUST_LOG\" \
    setup-args=\"$setupArgs\" \
    skip-setup=$skipSetup \
    leader-rotation=\"$leaderRotation\" \
  "

  if [[ -e /dev/nvidia0 ]]; then
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
    echo
    echo "WARNING: GPU detected by snap builds to not support CUDA."
    echo "         Consider using instances with a GPU to reduce cost."
    echo
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
  fi

  if [[ $nodeType = bootstrap-leader ]]; then
    nodeConfig="mode=bootstrap-leader+drone $commonNodeConfig"
    ln -sf -T /var/snap/solana/current/bootstrap-leader/current fullnode.log
    ln -sf -T /var/snap/solana/current/drone/current drone.log
  else
    nodeConfig="mode=fullnode $commonNodeConfig"
    ln -sf -T /var/snap/solana/current/fullnode/current fullnode.log
  fi

  logmarker="solana deploy $(date)/$RANDOM"
  logger "$logmarker"

  # shellcheck disable=SC2086,SC2090 # Don't want to double quote "$nodeConfig"
  sudo snap set solana $nodeConfig
  snap info solana
  sudo snap get solana
  echo Slight delay to get more syslog output
  sleep 2
  sudo grep -Pzo "$logmarker(.|\\n)*" /var/log/syslog

  echo "Succeeded in ${SECONDS} seconds"
  ;;
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

  maybeNoLeaderRotation=
  if ! $leaderRotation; then
    maybeNoLeaderRotation="--no-leader-rotation"
  fi

  rm -f init-complete-file
  case $nodeType in
  bootstrap-leader)
    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi
    set -x
    if [[ $skipSetup != true ]]; then
      ./multinode-demo/setup.sh -t bootstrap-leader $setupArgs
    fi
    ./multinode-demo/drone.sh > drone.log 2>&1 &
    ./multinode-demo/bootstrap-leader.sh \
      --init-complete-file init-complete-file
      $maybeNoLeaderRotation > bootstrap-leader.log 2>&1 &
    ln -sTf bootstrap-leader.log fullnode.log
    ;;
  fullnode)
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/.cargo/bin/ ~/.cargo/bin/

    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi

    set -x
    if [[ $skipSetup != true ]]; then
      ./multinode-demo/setup.sh -t fullnode $setupArgs
    fi
    ./multinode-demo/fullnode.sh \
      --init-complete-file init-complete-file \
      $maybeNoLeaderRotation \
      "$entrypointIp":~/solana "$entrypointIp:8001" > fullnode.log 2>&1 &
    ;;
  *)
    echo "Error: unknown node type: $nodeType"
    exit 1
    ;;
  esac

  SECONDS=0
  while [[ ! -r init-complete-file ]]; do
    if [[ $SECONDS -ge 30 ]]; then
      echo "Error: node failed to boot in $SECONDS seconds"
      exit 1
    fi
    echo "Waiting for node to boot ($SECONDS)..."
    sleep 2
  done
  echo "Node booted in $SECONDS seconds"

  disown
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac
