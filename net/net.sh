#!/bin/bash -e

here=$(dirname "$0")
SOLANA_ROOT="$(cd "$here"/..; pwd)"

# shellcheck source=net/common.sh
source "$here"/common.sh

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [start|stop]

Manage a multinode network

 start|stop    - Start or stop the network
EOF
  exit $exitcode
}

command=$1
[[ -n $command ]] || usage
shift
[[ $command = start || $command = stop ]] || usage "Invalid command: $command"

while getopts "h?" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

loadConfigFile

build() {
  declare MAYBE_DOCKER=
  if [[ $(uname) != Linux ]]; then
    MAYBE_DOCKER="ci/docker-run.sh solanalabs/rust"
  fi
  SECONDS=0
  (
    cd "$SOLANA_ROOT"
    echo "****************"
    echo "Build started at $(date)"

    set -x
    rm -rf farf
    $MAYBE_DOCKER cargo install --root farf
  )
  echo "Build took $SECONDS seconds"
}

common_start_setup() {
  declare ipAddress=$1
  declare logFile="$2"

  (
    set -x

    ssh "${sshOptions[@]}" "$ipAddress" "
      set -ex;
      sudo systemctl disable apt-daily.service # disable run when system boot
      sudo systemctl disable apt-daily.timer   # disable timer run
      sudo apt-get --assume-yes install rsync libssl-dev;
      mkdir -p ~/solana ~/.cargo/bin;
    "

    test -d "$SOLANA_ROOT"
    rsync -vPrz -e "ssh ${sshOptions[*]}" \
      "$SOLANA_ROOT"/{fetch-perf-libs.sh,scripts,net,multinode-demo} \
      "$ipAddress":~/solana/
  ) >> "$logFile"
}

startLeader() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "****************"
  echo "Starting leader: $leaderIp"

  common_start_setup "$ipAddress" "$logFile"

  (
    set -x
    rsync -vPrz -e "ssh ${sshOptions[*]}" "$SOLANA_ROOT"/farf/bin/* "$ipAddress":~/.cargo/bin/
    ssh "${sshOptions[@]}" -f "$ipAddress" \
      "./solana/net/remote/remote_leader.sh"
  ) >> "$logFile"
}

startValidator() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "*******************"
  echo "Starting validator: $leaderIp"
  common_start_setup "$ipAddress" "$logFile"

  (
    set -x
    ssh "${sshOptions[@]}" -f "$ipAddress" \
      "./solana/net/remote/remote_validator.sh $leaderIp"
   ) >> "$logFile"
}

startClient() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "****************"
  echo "Starting client: $leaderIp"
  common_start_setup "$ipAddress" "$logFile"

  ssh "${sshOptions[@]}" -f "$ipAddress" \
    "./solana/net/remote/remote_client.sh $leaderIp" >> "$logFile"
}

start() {
  echo "Deployment started at $(date)"
  SECONDS=0
  leaderDeployTime=

  startLeader "$leaderIp" "$netLogDir/leader-$leaderIp.log"
  leaderDeployTime=$SECONDS
  SECONDS=0

  for ipAddress in "${validatorIpList[@]}"; do
    startValidator "$ipAddress" "$netLogDir/validator-$ipAddress.log" &
  done

  wait
  validatorDeployTime=$SECONDS
  SECONDS=0

  for ipAddress in "${clientIpList[@]}"; do
    startClient "$ipAddress" "$netLogDir/client-$ipAddress.log"
  done

  clientDeployTime=$SECONDS
  SECONDS=0
  wait

  echo
  echo "================================================================="
  echo "Deployment finished at $(date)"
  echo "Leader deployment took $leaderDeployTime seconds"
  echo "Validator deployment (${#validatorIpList[@]} instances) took $validatorDeployTime seconds"
  echo "Client deployment (${#clientIpList[@]} instances) took $clientDeployTime seconds"
  echo "Logs in $netLogDir:"
  ls -l "$netLogDir"
}


stop_node() {
  local ipAddress=$1
  echo "**************"
  echo "Stopping node: $ipAddress"
  (
    set -x
    ssh "${sshOptions[@]}" "$ipAddress" "
      set -x;
      pkill -9 solana- remote_ oom-monitor
    "
  ) || true
}

stop() {
  SECONDS=0

  stop_node "$leaderIp"

  for ipAddress in "${validatorIpList[@]}" "${clientIpList[@]}"; do
    stop_node "$ipAddress"
  done

  echo "Stopping nodes took $SECONDS seconds"
}

mkdir -p log

if [[ $command == "start" ]]; then
  build
  stop
  start
elif [[ $command == "stop" ]]; then
  stop
else
  usage "Unknown command: $command"
fi
