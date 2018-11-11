#!/usr/bin/env bash -e

cd "$(dirname "$0")"/../..

deployMethod="$1"
nodeType="$2"
publicNetwork="$3"
entrypointIp="$4"
numNodes="$5"
RUST_LOG="$6"

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]]  || missing deployMethod
[[ -n $nodeType ]]      || missing nodeType
[[ -n $publicNetwork ]] || missing publicNetwork
[[ -n $entrypointIp ]]  || missing entrypointIp
[[ -n $numNodes ]]      || missing numNodes

cat > deployConfig <<EOF
deployMethod="$deployMethod"
entrypointIp="$entrypointIp"
numNodes="$numNodes"
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
  [[ $nodeType = leader ]] ||
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/solana/solana.snap" .
  sudo snap install solana.snap --devmode --dangerous

  # shellcheck disable=SC2089
  commonNodeConfig="\
    leader-ip=\"$entrypointIp\" \
    default-metrics-rate=1 \
    metrics-config=\"$SOLANA_METRICS_CONFIG\" \
    rust-log=\"$RUST_LOG\" \
    setup-args=\"$setupArgs\" \
  "

  if [[ -e /dev/nvidia0 ]]; then
    commonNodeConfig="$commonNodeConfig enable-cuda=1"
  fi

  if [[ $nodeType = leader ]]; then
    nodeConfig="mode=leader+drone $commonNodeConfig"
    ln -sf -T /var/snap/solana/current/leader/current leader.log
    ln -sf -T /var/snap/solana/current/drone/current drone.log
  else
    nodeConfig="mode=validator $commonNodeConfig"
    ln -sf -T /var/snap/solana/current/validator/current validator.log
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
  export SOLANA_DEFAULT_METRICS_RATE=1

  ./fetch-perf-libs.sh
  export LD_LIBRARY_PATH="$PWD/target/perf-libs:/usr/local/cuda/lib64:$LD_LIBRARY_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"

  scripts/oom-monitor.sh  > oom-monitor.log 2>&1 &
  scripts/net-stats.sh  > net-stats.log 2>&1 &

  case $nodeType in
  leader)
    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi
    ./multinode-demo/setup.sh -t leader $setupArgs
    ./multinode-demo/drone.sh > drone.log 2>&1 &
    ./multinode-demo/leader.sh > leader.log 2>&1 &
    ;;
  validator)
    net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/.cargo/bin/solana*" ~/.cargo/bin/

    if [[ -e /dev/nvidia0 && -x ~/.cargo/bin/solana-fullnode-cuda ]]; then
      echo Selecting solana-fullnode-cuda
      export SOLANA_CUDA=1
    fi

    ./multinode-demo/setup.sh -t validator $setupArgs
    ./multinode-demo/validator.sh "$entrypointIp":~/solana "$entrypointIp:8001" >validator.log 2>&1 &
    ;;
  *)
    echo "Error: unknown node type: $nodeType"
    exit 1
    ;;
  esac
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac
