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

Operate a configured testnet

 start - Start the network
 sanity - Sanity check the network
 stop  - Stop the network

 start-specific options:
   -S snapFilename      - Deploy the specified Snap file
   -s edge|beta|stable  - Deploy the latest Snap on the specified Snap release channel
   -a "setup args"      - Optional additional arguments for ./multinode-demo/setup.sh

   Note: if RUST_LOG is set in the environment it will be propogated into the
         network nodes.

 sanity-specific options:
   -o noLedgerVerify    - Skip ledger verification
   -o noValidatorSanity - Skip validatory sanity

 stop-specific options:
   none

EOF
  exit $exitcode
}

snapChannel=
snapFilename=
nodeSetupArgs=
deployMethod=local
sanityExtraArgs=

command=$1
[[ -n $command ]] || usage
shift
[[ $command = start || $command = sanity || $command = stop ]] ||
  usage "Invalid command: $command"

while getopts "h?S:s:a:o:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  S)
    [[ $command = start ]] || usage "-s is only valid with the 'start' command"
    snapFilename=$OPTARG
    [[ -f $snapFilename ]] || usage "Snap not readable: $snapFilename"
    deployMethod=snap
    ;;
  s)
    case $OPTARG in
    edge|beta|stable)
      snapChannel=$OPTARG
      ;;
    *)
      usage "Invalid snap channel: $OPTARG"
      ;;
    esac
    ;;
  a)
    nodeSetupArgs="$OPTARG"
    ;;
  o)
    case $OPTARG in
    noLedgerVerify|noValidatorSanity)
      sanityExtraArgs="$sanityExtraArgs -o $OPTARG"
      ;;
    *)
      echo "Error: unknown option: $OPTARG"
      exit 1
      ;;
    esac
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
    test -d "$SOLANA_ROOT"
    ssh "${sshOptions[@]}" "$ipAddress" "mkdir -p ~/solana ~/.cargo/bin"
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

  # Deploy local binaries to leader.  Validators and clients later fetch the
  # binaries from the leader.
  (
    set -x
    case $deployMethod in
    snap)
      rsync -vPrz -e "ssh ${sshOptions[*]}" "$snapFilename" "$ipAddress:~/solana/solana.snap"
      ;;
    local)
      rsync -vPrz -e "ssh ${sshOptions[*]}" "$SOLANA_ROOT"/farf/bin/* "$ipAddress:~/.cargo/bin/"
      ;;
    *)
      usage "Internal error: invalid deployMethod: $deployMethod"
      ;;
    esac

    ssh "${sshOptions[@]}" -f "$ipAddress" \
      "./solana/net/remote/remote_node.sh $deployMethod leader $leaderIp \"$nodeSetupArgs\" \"$RUST_LOG\""
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
      "./solana/net/remote/remote_node.sh $deployMethod validator $leaderIp \"$nodeSetupArgs\" \"$RUST_LOG\""
  ) >> "$logFile"
}

startClient() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "****************"
  echo "Starting client: $leaderIp"
  common_start_setup "$ipAddress" "$logFile"

  declare expectedNodeCount=$((${#validatorIpList[@]} + 1))

  (
    set -x
    ssh "${sshOptions[@]}" -f "$ipAddress" \
      "./solana/net/remote/remote_client.sh $deployMethod $leaderIp $expectedNodeCount \"$RUST_LOG\""
  ) >> "$logFile"
}

sanity() {
  declare expectedNodeCount=$((${#validatorIpList[@]} + 1))
  (
    set -x
    # shellcheck disable=SC2029 # remote_client.sh are expanded on client side intentionally...
    ssh "${sshOptions[@]}" "$leaderIp" \
      "./solana/net/remote/remote_sanity.sh $deployMethod $leaderIp $expectedNodeCount $sanityExtraArgs"
  )
}

start() {
  case $deployMethod in
  snap)
    if [[ -n $snapChannel ]]; then
      if [[ $(uname) != Linux ]]; then
        echo Error: snap channel deployment only supported in Linux
        exit 1
      fi
      usage "TODO: the snap download command below is probably wrong..."
      snap download --"$snapChannel" solana
      snapFilename=solana.snap
    fi
    ;;
  local)
    build
    ;;
  *)
    usage "Internal error: invalid deployMethod: $deployMethod"
    ;;
  esac

  echo "Deployment started at $(date)"

  SECONDS=0
  declare leaderDeployTime=
  declare networkVersion=unknown
  startLeader "$leaderIp" "$netLogDir/leader-$leaderIp.log"
  leaderDeployTime=$SECONDS

  SECONDS=0
  for ipAddress in "${validatorIpList[@]}"; do
    startValidator "$ipAddress" "$netLogDir/validator-$ipAddress.log" &
  done
  wait
  validatorDeployTime=$SECONDS

  sanity

  SECONDS=0
  for ipAddress in "${clientIpList[@]}"; do
    startClient "$ipAddress" "$netLogDir/client-$ipAddress.log"
  done
  clientDeployTime=$SECONDS
  wait

  if [[ $deployMethod = "snap" ]]; then
    IFS=\  read -r _ networkVersion _ < <(
      ssh "${sshOptions[@]}" "$leaderIp" \
        "snap info solana | grep \"^installed:\""
    )
    networkVersion=${networkVersion/0+git./}
  fi

  $metricsWriteDatapoint "testnet-deploy,name=$netBasename start=1,version=\"$networkVersion\""

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
      if snap list solana; then
        sudo snap set solana mode=;
        sudo snap remove solana;
      fi; \
      pkill -9 solana- remote_ oom-monitor;
    "
  ) || true
}

stop() {
  SECONDS=0

  $metricsWriteDatapoint "testnet-deploy,name=$netBasename stop=1"

  stop_node "$leaderIp"

  for ipAddress in "${validatorIpList[@]}" "${clientIpList[@]}"; do
    stop_node "$ipAddress"
  done

  echo "Stopping nodes took $SECONDS seconds"
}

case $command in
start)
  stop
  start
  ;;
sanity)
  sanity
  ;;
stop)
  stop
  ;;
*)
  echo "Internal error: Unknown command: $command"
  exit 1
esac
