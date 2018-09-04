#!/bin/bash -e

deployMethod="$1"
nodeType="$2"
netEntrypoint="$3"
setupArgs="$4"
RUST_LOG="$5"

[[ -n $deployMethod ]] || exit
[[ -n $nodeType ]] || exit
[[ -n $netEntrypoint ]] || exit

cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

./script/install-earlyoom.sh

case $deployMethod in
snap)
  SECONDS=0
  sudo snap install solana.snap --devmode --dangerous
  rm solana.snap

  commonNodeConfig="\
    rust-log=$RUST_LOG \
    metrics-config=$SOLANA_METRICS_CONFIG \
    setup-args=$setupArgs \
    enable-cuda=1 \
  "
  if [[ $nodeType = leader ]]; then
    nodeConfig="mode=leader+drone $commonNodeConfig"
  else
    nodeConfig="mode=validator leader-address=$netEntrypoint $commonNodeConfig"
  fi

  logmarker="solana deploy $(date)/$RANDOM"
  logger "$logmarker"

  # shellcheck disable=SC2086 # Don't want to double quote "$nodeConfig"
  sudo snap set solana $nodeConfig
  snap info solana
  sudo snap get solana
  echo Slight delay to get more syslog output
  sleep 2
  sudo grep -Pzo "$logmarker(.|\\n)*" /var/log/syslog

  echo "Succeeded in ${SECONDS} seconds"
  ;;
local)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export SOLANA_CUDA=1
  export RUST_LOG=1

  ./fetch-perf-libs.sh
  ./scripts/oom-monitor.sh  > oom-monitor.log 2>&1 &

  case $nodeType in
  leader)
    # shellcheck disable=SC2086 # Don't want to double quote "$setupArgs"
    ./multinode-demo/setup.sh -t leader -p $setupArgs
    ./multinode-demo/drone.sh > drone.log 2>&1 &
    ./multinode-demo/leader.sh > leader.log 2>&1 &
    ;;
  validator)
    rsync -vPrz "$netEntrypoint:~/.cargo/bin/solana*" ~/.cargo/bin/

    # shellcheck disable=SC2086 # Don't want to double quote "$setupArgs"
    ./multinode-demo/setup.sh -t validator -p $setupArgs
    ./multinode-demo/validator.sh "$netEntrypoint":~/solana "$netEntrypoint" >validator.log 2>&1 &
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

