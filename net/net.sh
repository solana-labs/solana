#!/usr/bin/env bash
set -e

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
 update   - Live update all network nodes

 start/update-specific options:
   -S [snapFilename]           - Deploy the specified Snap file
   -s edge|beta|stable         - Deploy the latest Snap on the specified Snap release channel
   -T [tarFilename]            - Deploy the specified release tarball
   -t edge|beta|stable|vX.Y.Z  - Deploy the latest tarball release for the
                                 specified release channel (edge|beta|stable) or release tag
                                 (vX.Y.Z)
   -f [cargoFeatures]          - List of |cargo --feaures=| to activate
                                 (ignored if -s or -S is specified)
   -r                          - Reuse existing node/ledger configuration from a
                                 previous |start| (ie, don't run ./mulitnode-demo/setup.sh).
   -D /path/to/programs        - Deploy custom programs from this location

   Note: if RUST_LOG is set in the environment it will be propogated into the
         network nodes.

 sanity/start/update-specific options:
   -o noLedgerVerify    - Skip ledger verification
   -o noValidatorSanity - Skip fullnode sanity
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
skipSetup=false
updateNodes=false
customPrograms=

command=$1
[[ -n $command ]] || usage
shift

while getopts "h?S:s:T:t:o:f:r:D:" opt; do
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
  T)
    tarballFilename=$OPTARG
    [[ -f $tarballFilename ]] || usage "Snap not readable: $tarballFilename"
    deployMethod=tar
    ;;
  t)
    case $OPTARG in
    edge|beta|stable|v*)
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
  r)
    skipSetup=true
    ;;
  D)
    customPrograms=$OPTARG
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

    if [[ -r target/perf-libs/env.sh ]]; then
      # shellcheck source=/dev/null
      source target/perf-libs/env.sh
    fi
    $MAYBE_DOCKER bash -c "
      set -ex
      scripts/cargo-install-all.sh farf \"$cargoFeatures\"
      if [[ -n \"$customPrograms\" ]]; then
        scripts/cargo-install-custom-programs.sh farf $customPrograms
      fi
    "
  )
  echo "Build took $SECONDS seconds"
}

startCommon() {
  declare ipAddress=$1
  test -d "$SOLANA_ROOT"
  ssh "${sshOptions[@]}" "$ipAddress" "mkdir -p ~/solana ~/.cargo/bin; rm -rf ~/solana/"
  rsync -vPrc -e "ssh ${sshOptions[*]}" \
    "$SOLANA_ROOT"/{fetch-perf-libs.sh,scripts,net,multinode-demo} \
    "$ipAddress":~/solana/
}

startBootstrapLeader() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "--- Starting bootstrap leader: $ipAddress"
  echo "start log: $logFile"

  # Deploy local binaries to bootstrap fullnode.  Other fullnodes and clients later fetch the
  # binaries from it
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
      "./solana/net/remote/remote-node.sh \
         $deployMethod \
         bootstrap-leader \
         $publicNetwork \
         $entrypointIp \
         ${#fullnodeIpList[@]} \
         \"$RUST_LOG\" \
         $skipSetup \
         $leaderRotation \
      "
  ) >> "$logFile" 2>&1 || {
    cat "$logFile"
    echo "^^^ +++"
    exit 1
  }
}

startNode() {
  declare ipAddress=$1
  declare logFile="$netLogDir/fullnode-$ipAddress.log"

  echo "--- Starting fullnode: $ipAddress"
  echo "start log: $logFile"
  (
    set -x
    startCommon "$ipAddress"
    ssh "${sshOptions[@]}" -n "$ipAddress" \
      "./solana/net/remote/remote-node.sh \
         $deployMethod \
         fullnode \
         $publicNetwork \
         $entrypointIp \
         ${#fullnodeIpList[@]} \
         \"$RUST_LOG\" \
         $skipSetup \
         $leaderRotation \
      "
  ) >> "$logFile" 2>&1 &
  declare pid=$!
  ln -sfT "fullnode-$ipAddress.log" "$netLogDir/fullnode-$pid.log"
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
      "./solana/net/remote/remote-client.sh $deployMethod $entrypointIp \"$RUST_LOG\""
  ) >> "$logFile" 2>&1 || {
    cat "$logFile"
    echo "^^^ +++"
    exit 1
  }
}

sanity() {
  declare ok=true

  echo "--- Sanity"
  $metricsWriteDatapoint "testnet-deploy net-sanity-begin=1"

  declare host=${fullnodeIpList[0]}
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
      (
        set -x
        curl -o "$SOLANA_ROOT"/solana-release.tar.bz2 http://solana-release.s3.amazonaws.com/"$releaseChannel"/solana-release.tar.bz2
      )
      tarballFilename="$SOLANA_ROOT"/solana-release.tar.bz2
    fi
    (
      set -x
      rm -rf "$SOLANA_ROOT"/solana-release
      (cd "$SOLANA_ROOT"; tar jxv) < "$tarballFilename"
      cat "$SOLANA_ROOT"/solana-release/version.txt
    )
    ;;
  local)
    build
    ;;
  *)
    usage "Internal error: invalid deployMethod: $deployMethod"
    ;;
  esac

  echo "Deployment started at $(date)"
  if $updateNodes; then
    $metricsWriteDatapoint "testnet-deploy net-update-begin=1"
  else
    $metricsWriteDatapoint "testnet-deploy net-start-begin=1"
  fi

  bootstrapLeader=true
  for ipAddress in "${fullnodeIpList[@]}"; do
    if $updateNodes; then
      stopNode "$ipAddress"
    fi
    if $bootstrapLeader; then
      SECONDS=0
      declare bootstrapNodeDeployTime=
      startBootstrapLeader "$ipAddress" "$netLogDir/bootstrap-leader-$ipAddress.log"
      bootstrapNodeDeployTime=$SECONDS
      $metricsWriteDatapoint "testnet-deploy net-bootnode-leader-started=1"

      bootstrapLeader=false
      SECONDS=0
      pids=()
      loopCount=0
    else
      startNode "$ipAddress"

      # Stagger additional node start time. If too many nodes start simultaneously
      # the bootstrap node gets more rsync requests from the additional nodes than
      # it can handle.
      ((loopCount++ % 2 == 0)) && sleep 2
    fi
  done

  for pid in "${pids[@]}"; do
    declare ok=true
    wait "$pid" || ok=false
    if ! $ok; then
      cat "$netLogDir/fullnode-$pid.log"
      echo ^^^ +++
      exit 1
    fi
  done

  $metricsWriteDatapoint "testnet-deploy net-fullnodes-started=1"
  additionalNodeDeployTime=$SECONDS

  if ! $updateNodes; then
    sanity
  fi

  SECONDS=0
  for ipAddress in "${clientIpList[@]}"; do
    if $updateNodes; then
      stopNode "$ipAddress"
    fi
    startClient "$ipAddress" "$netLogDir/client-$ipAddress.log"
  done
  clientDeployTime=$SECONDS

  if $updateNodes; then
    $metricsWriteDatapoint "testnet-deploy net-update-complete=1"
  else
    $metricsWriteDatapoint "testnet-deploy net-start-complete=1"
  fi

  declare networkVersion=unknown
  case $deployMethod in
  snap)
    IFS=\  read -r _ networkVersion _ < <(
      ssh "${sshOptions[@]}" "${fullnodeIpList[0]}" \
        "snap info solana | grep \"^installed:\""
    )
    networkVersion=${networkVersion/0+git./}
    ;;
  tar)
    networkVersion="$(
      tail -n1 "$SOLANA_ROOT"/solana-release/version.txt || echo "tar-unknown"
    )"
    ;;
  local)
    networkVersion="$(git rev-parse HEAD || echo local-unknown)"
    ;;
  *)
    usage "Internal error: invalid deployMethod: $deployMethod"
    ;;
  esac
  $metricsWriteDatapoint "testnet-deploy version=\"${networkVersion:0:9}\""

  echo
  echo "+++ Deployment Successful"
  echo "Bootstrap leader deployment took $bootstrapNodeDeployTime seconds"
  echo "Additional fullnode deployment (${#fullnodeIpList[@]} instances) took $additionalNodeDeployTime seconds"
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

  for ipAddress in "${fullnodeIpList[@]}" "${clientIpList[@]}"; do
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
update)
  $leaderRotation || {
    echo Warning: unable to update because leader rotation is disabled
    exit 1
  }
  skipSetup=true
  updateNodes=true
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
