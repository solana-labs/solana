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
usage: $0 [start|stop|restart|sanity] [command-specific options]

Operate a configured testnet

 start    - Start the network
 sanity   - Sanity check the network
 stop     - Stop the network
 restart  - Shortcut for stop then start

 start-specific options:
   -S [snapFilename]    - Deploy the specified Snap file
   -s edge|beta|stable  - Deploy the latest Snap on the specified Snap release channel
   -t edge|beta|stable  - Deploy the latest tarball release for the specified channel
   -f [cargoFeatures]   - List of |cargo --feaures=| to activate
                          (ignored if -s or -S is specified)

   Note: if RUST_LOG is set in the environment it will be propogated into the
         network nodes.

 sanity/start-specific options:
   -o noLedgerVerify    - Skip ledger verification
   -o noValidatorSanity - Skip validator sanity
   -o rejectExtraNodes  - Require the exact number of nodes

 stop-specific options:
   none

EOF
  exit $exitcode
}

snapChannel=
releaseChannel=
snapFilename=
deployMethod=local
sanityExtraArgs=
cargoFeatures=

command=$1
[[ -n $command ]] || usage
shift

while getopts "h?S:s:t:o:f:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  S)
    snapFilename=$OPTARG
    [[ -f $snapFilename ]] || usage "Snap not readable: $snapFilename"
    deployMethod=snap
    ;;
  s)
    case $OPTARG in
    edge|beta|stable)
      snapChannel=$OPTARG
      deployMethod=snap
      ;;
    *)
      usage "Invalid snap channel: $OPTARG"
      ;;
    esac
    ;;
  t)
    case $OPTARG in
    edge|beta|stable)
      releaseChannel=$OPTARG
      deployMethod=tar
      ;;
    *)
      usage "Invalid release channel: $OPTARG"
      ;;
    esac
    ;;
  f)
    cargoFeatures=$OPTARG
    ;;
  o)
    case $OPTARG in
    noLedgerVerify|noValidatorSanity|rejectExtraNodes)
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
expectedNodeCount=$((${#validatorIpList[@]} + 1))

build() {
  declare MAYBE_DOCKER=
  if [[ $(uname) != Linux ]]; then
    MAYBE_DOCKER="ci/docker-run.sh solanalabs/rust"
  fi
  SECONDS=0
  (
    cd "$SOLANA_ROOT"
    echo "--- Build started at $(date)"

    set -x
    rm -rf farf
    $MAYBE_DOCKER cargo install --features="$cargoFeatures" --root farf
    ./scripts/install-native-programs.sh farf/
  )
  echo "Build took $SECONDS seconds"
}

startCommon() {
  declare ipAddress=$1
  test -d "$SOLANA_ROOT"
  ssh "${sshOptions[@]}" "$ipAddress" "mkdir -p ~/solana ~/.cargo/bin"
  rsync -vPrc -e "ssh ${sshOptions[*]}" \
    "$SOLANA_ROOT"/{fetch-perf-libs.sh,scripts,net,multinode-demo} \
    "$ipAddress":~/solana/
}

startLeader() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "--- Starting leader: $leaderIp"
  echo "start log: $logFile"

  # Deploy local binaries to leader.  Validators and clients later fetch the
  # binaries from the leader.
  (
    set -x
    startCommon "$ipAddress" || exit 1
    case $deployMethod in
    snap)
      rsync -vPrc -e "ssh ${sshOptions[*]}" "$snapFilename" "$ipAddress:~/solana/solana.snap"
      ;;
    tar)
      rsync -vPrc -e "ssh ${sshOptions[*]}" "$SOLANA_ROOT"/solana-release/bin/* "$ipAddress:~/.cargo/bin/"
      ;;
    local)
      rsync -vPrc -e "ssh ${sshOptions[*]}" "$SOLANA_ROOT"/farf/bin/* "$ipAddress:~/.cargo/bin/"
      ;;
    *)
      usage "Internal error: invalid deployMethod: $deployMethod"
      ;;
    esac

    ssh "${sshOptions[@]}" -n "$ipAddress" \
      "./solana/net/remote/remote-node.sh $deployMethod leader $publicNetwork $entrypointIp $expectedNodeCount \"$RUST_LOG\""
  ) >> "$logFile" 2>&1 || {
    cat "$logFile"
    echo "^^^ +++"
    exit 1
  }
}

startValidator() {
  declare ipAddress=$1
  declare logFile="$netLogDir/validator-$ipAddress.log"

  echo "--- Starting validator: $ipAddress"
  echo "start log: $logFile"
  (
    set -x
    startCommon "$ipAddress"
    ssh "${sshOptions[@]}" -n "$ipAddress" \
      "./solana/net/remote/remote-node.sh $deployMethod validator $publicNetwork $entrypointIp $expectedNodeCount \"$RUST_LOG\""
  ) >> "$logFile" 2>&1 &
  declare pid=$!
  ln -sfT "validator-$ipAddress.log" "$netLogDir/validator-$pid.log"
  pids+=("$pid")
}

startClient() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "--- Starting client: $ipAddress"
  echo "start log: $logFile"
  (
    set -x
    startCommon "$ipAddress"
    ssh "${sshOptions[@]}" -f "$ipAddress" \
      "./solana/net/remote/remote-client.sh $deployMethod $entrypointIp $expectedNodeCount \"$RUST_LOG\""
  ) >> "$logFile" 2>&1 || {
    cat "$logFile"
    echo "^^^ +++"
    exit 1
  }
}

sanity() {
  declare expectedNodeCount=$((${#validatorIpList[@]} + 1))
  declare ok=true

  echo "--- Sanity"
  $metricsWriteDatapoint "testnet-deploy net-sanity-begin=1"

  declare host=$leaderIp # TODO: maybe use ${validatorIpList[0]} ?
  (
    set -x
    # shellcheck disable=SC2029 # remote-client.sh args are expanded on client side intentionally
    ssh "${sshOptions[@]}" "$host" \
      "./solana/net/remote/remote-sanity.sh $sanityExtraArgs"
  ) || ok=false

  $metricsWriteDatapoint "testnet-deploy net-sanity-complete=1"
  $ok || exit 1
}

start() {
  case $deployMethod in
  snap)
    if [[ -n $snapChannel ]]; then
      rm -f "$SOLANA_ROOT"/solana_*.snap
      if [[ $(uname) != Linux ]]; then
        (
          set -x
          SOLANA_DOCKER_RUN_NOSETUID=1 "$SOLANA_ROOT"/ci/docker-run.sh ubuntu:18.04 bash -c "
            set -ex;
            apt-get -qq update;
            apt-get -qq -y install snapd;
            until snap download --channel=$snapChannel solana; do
              sleep 1;
            done
          "
        )
      else
        (
          cd "$SOLANA_ROOT"
          until snap download --channel="$snapChannel" solana; do
            sleep 1
          done
        )
      fi
      snapFilename="$(echo "$SOLANA_ROOT"/solana_*.snap)"
      [[ -r $snapFilename ]] || {
        echo "Error: Snap not readable: $snapFilename"
        exit 1
      }
    fi
    ;;
  tar)
    if [[ -n $releaseChannel ]]; then
      rm -f "$SOLANA_ROOT"/solana-release.tar.bz2
      cd "$SOLANA_ROOT"

      set -x
      curl -o solana-release.tar.bz2 http://solana-release.s3.amazonaws.com/"$releaseChannel"/solana-release.tar.bz2
      tar jxvf solana-release.tar.bz2
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
  $metricsWriteDatapoint "testnet-deploy net-start-begin=1"

  SECONDS=0
  declare leaderDeployTime=
  startLeader "$leaderIp" "$netLogDir/leader-$leaderIp.log"
  leaderDeployTime=$SECONDS
  $metricsWriteDatapoint "testnet-deploy net-leader-started=1"

  SECONDS=0
  pids=()
  loopCount=0
  for ipAddress in "${validatorIpList[@]}"; do
    startValidator "$ipAddress"

    # Staggering validator startup time. If too many validators
    # bootup simultaneously, leader node gets more rsync requests
    # from the validators than it can handle.
    ((loopCount++ % 2 == 0)) && sleep 2
  done

  for pid in "${pids[@]}"; do
    declare ok=true
    wait "$pid" || ok=false
    if ! $ok; then
      cat "$netLogDir/validator-$pid.log"
      echo ^^^ +++
      exit 1
    fi
  done

  $metricsWriteDatapoint "testnet-deploy net-validators-started=1"
  validatorDeployTime=$SECONDS

  sanity

  SECONDS=0
  for ipAddress in "${clientIpList[@]}"; do
    startClient "$ipAddress" "$netLogDir/client-$ipAddress.log"
  done
  clientDeployTime=$SECONDS
  $metricsWriteDatapoint "testnet-deploy net-start-complete=1"

  if [[ $deployMethod = "snap" ]]; then
    declare networkVersion=unknown
    IFS=\  read -r _ networkVersion _ < <(
      ssh "${sshOptions[@]}" "$leaderIp" \
        "snap info solana | grep \"^installed:\""
    )
    networkVersion=${networkVersion/0+git./}
    $metricsWriteDatapoint "testnet-deploy version=\"$networkVersion\""
  fi

  echo
  echo "+++ Deployment Successful"
  echo "Leader deployment took $leaderDeployTime seconds"
  echo "Validator deployment (${#validatorIpList[@]} instances) took $validatorDeployTime seconds"
  echo "Client deployment (${#clientIpList[@]} instances) took $clientDeployTime seconds"
  echo "Network start logs in $netLogDir:"
  ls -l "$netLogDir"
}


stopNode() {
  local ipAddress=$1
  echo "--- Stopping node: $ipAddress"
  (
    set -x
    ssh "${sshOptions[@]}" "$ipAddress" "
      set -x
      if snap list solana; then
        sudo snap set solana mode=
        sudo snap remove solana
      fi
      ! tmux list-sessions || tmux kill-session
      for pattern in solana- remote- oom-monitor net-stats; do
        pkill -9 \$pattern
      done
    "
  ) || true
}

stop() {
  SECONDS=0
  $metricsWriteDatapoint "testnet-deploy net-stop-begin=1"

  stopNode "$leaderIp"

  for ipAddress in "${validatorIpList[@]}" "${clientIpList[@]}"; do
    stopNode "$ipAddress"
  done

  $metricsWriteDatapoint "testnet-deploy net-stop-complete=1"
  echo "Stopping nodes took $SECONDS seconds"
}

case $command in
restart)
  stop
  start
  ;;
start)
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
