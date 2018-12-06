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

 start-specific options:
   -S [snapFilename]           - Deploy the specified Snap file
   -s edge|beta|stable         - Deploy the latest Snap on the specified Snap release channel
   -T [tarFilename]            - Deploy the specified release tarball
   -t edge|beta|stable|vX.Y.Z  - Deploy the latest tarball release for the
                                 specified release channel (edge|beta|stable) or release tag
                                 (vX.Y.Z)
   -f [cargoFeatures]          - List of |cargo --feaures=| to activate
                                 (ignored if -s or -S is specified)

   Note: if RUST_LOG is set in the environment it will be propogated into the
         network nodes.

 sanity/start-specific options:
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

command=$1
[[ -n $command ]] || usage
shift

while getopts "h?S:s:T:t:o:f:" opt; do
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
expectedNodeCount=$((${#additionalFullNodeIps[@]} + 1))

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
      export NDEBUG=1
      cargo install --path drone --features=$cargoFeatures --root farf
      cargo install --path . --features=$cargoFeatures --root farf
    "
    ./scripts/install-native-programs.sh farf/
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

startBootstrapNode() {
  declare ipAddress=$1
  declare logFile="$2"
  echo "--- Starting bootstrap full node: $bootstrapFullNodeIp"
  echo "start log: $logFile"

  # Deploy local binaries to bootstrap full node.  Other full nodes and clients later fetch the
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
      "./solana/net/remote/remote-node.sh $deployMethod bootstrap_fullnode $publicNetwork $entrypointIp $expectedNodeCount \"$RUST_LOG\""
  ) >> "$logFile" 2>&1 || {
    cat "$logFile"
    echo "^^^ +++"
    exit 1
  }
}

startNode() {
  declare ipAddress=$1
  declare logFile="$netLogDir/fullnode-$ipAddress.log"

  echo "--- Starting full node: $ipAddress"
  echo "start log: $logFile"
  (
    set -x
    startCommon "$ipAddress"
    ssh "${sshOptions[@]}" -n "$ipAddress" \
      "./solana/net/remote/remote-node.sh $deployMethod fullnode $publicNetwork $entrypointIp $expectedNodeCount \"$RUST_LOG\""
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
  declare expectedNodeCount=$((${#additionalFullNodeIps[@]} + 1))
  declare ok=true

  echo "--- Sanity"
  $metricsWriteDatapoint "testnet-deploy net-sanity-begin=1"

  declare host=$bootstrapFullNodeIp # TODO: maybe use ${additionalFullNodeIps[0]} ?
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
  $metricsWriteDatapoint "testnet-deploy net-start-begin=1"

  SECONDS=0
  declare bootstrapNodeDeployTime=
  startBootstrapNode "$bootstrapFullNodeIp" "$netLogDir/bootstrap-fullnode-$bootstrapFullNodeIp.log"
  bootstrapNodeDeployTime=$SECONDS
  $metricsWriteDatapoint "testnet-deploy net-leader-started=1"

  SECONDS=0
  pids=()
  loopCount=0
  for ipAddress in "${additionalFullNodeIps[@]}"; do
    startNode "$ipAddress"

    # Stagger additional node start time. If too many nodes start simultaneously
    # the bootstrap node gets more rsync requests from the additional nodes than
    # it can handle.
    ((loopCount++ % 2 == 0)) && sleep 2
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

  $metricsWriteDatapoint "testnet-deploy net-validators-started=1"
  additionalNodeDeployTime=$SECONDS

  sanity

  SECONDS=0
  for ipAddress in "${clientIpList[@]}"; do
    startClient "$ipAddress" "$netLogDir/client-$ipAddress.log"
  done
  clientDeployTime=$SECONDS
  $metricsWriteDatapoint "testnet-deploy net-start-complete=1"

  declare networkVersion=unknown
  case $deployMethod in
  snap)
    IFS=\  read -r _ networkVersion _ < <(
      ssh "${sshOptions[@]}" "$bootstrapFullNodeIp" \
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
  echo "Bootstrap full node deployment took $bootstrapNodeDeployTime seconds"
  echo "Additional full node deployment (${#additionalFullNodeIps[@]} instances) took $additionalNodeDeployTime seconds"
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

  stopNode "$bootstrapFullNodeIp"

  for ipAddress in "${additionalFullNodeIps[@]}" "${clientIpList[@]}"; do
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
