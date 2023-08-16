#!/usr/bin/env bash
set -e

here=$(dirname "$0")
SOLANA_ROOT="$(cd "$here"/..; pwd)"
# shellcheck source=net/common.sh
source "$here"/common.sh

namespace=testnet-dev-${USER//[^A-Za-z0-9]/}
deployMethod=local
doBuild=true
debugBuild=false
profileBuild=false
deployIfNewer=
skipSetup=false

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|delete] [common options]

Manage monogon instances
 start        - Start the network
 stop         - Stop the network

 common options:
   -p [namespace]      - Optional namespace for deployment
   -n [number]         - Number of additional validators (default: $additionalValidatorCount)
EOF
  exit $exitcode
}

initLogDir() { # Initializes the netLogDir global variable.  Idempotent
  [[ -z $netLogDir ]] || return 0

  netLogDir="$netDir"/log
  declare netLogDateDir
  netLogDateDir="$netDir"/log-$(date +"%Y-%m-%d_%H_%M_%S")
  if [[ -d $netLogDir && ! -L $netLogDir ]]; then
    echo "Warning: moving $netLogDir to make way for symlink."
    mv "$netLogDir" "$netDir"/log.old
  elif [[ -L $netLogDir ]]; then
    rm "$netLogDir"
  fi
  mkdir -p "$netConfigDir" "$netLogDateDir"
  ln -sf "$netLogDateDir" "$netLogDir"
  echo "Log directory: $netLogDateDir"
}

stop() {
    namespaceStatus=$(kubectl get ns "$namespace" -o json --ignore-not-found | jq .status.phase -r)
    if [ "$namespaceStatus" = "Active" ]
    then
        echo "Deleting namespace $namespace ..."
        kubectl delete ns "$namespace"
    else 
        echo "Namespace $namespace doesn't exist"
    fi
}

deploy() {
    $metricsWriteDatapoint "testnet-deploy net-create-begin=1"

    echo "Creating Namespace $namespace..."
    kubectl create ns "$namespace" 

    echo "Deploying bootstrap deployment/service"
    deployBootstrapValidatorDeployments

    echo "Deploying validator deployments/services"
    deployValidatorDeployments

    buildInstanceStartupScript

    echo "installing required libraries..."
    installRequiredLibraries

    $metricsWriteDatapoint "testnet-deploy net-create-complete=1"

}

build() {
  supported=("20.04")
  SECONDS=0
  (
    cd "$SOLANA_ROOT"
    echo "--- Build started at $(date)"

    set -x
    rm -rf farf

    buildVariant=
    if $debugBuild; then
      buildVariant=--debug
    fi

    if $profileBuild; then
      profilerFlags="RUSTFLAGS='-C force-frame-pointers=y -g ${RUSTFLAGS}'"
    fi

    $MAYBE_DOCKER bash -c "
      set -ex
      $profilerFlags scripts/cargo-install-all.sh farf $buildVariant --validator-only
    "
  )

  (
    set +e
    COMMIT="$(git rev-parse HEAD)"
    BRANCH="$(git rev-parse --abbrev-ref HEAD)"
    TAG="$(git describe --exact-match --tags HEAD 2>/dev/null)"
    if [[ $TAG =~ ^v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
      NOTE=$TAG
    else
      NOTE=$BRANCH
    fi
    (
      echo "channel: devbuild $NOTE"
      echo "commit: $COMMIT"
    ) > "$SOLANA_ROOT"/farf/version.yml
  )
  echo "Build took $SECONDS seconds"
}


prepareDeploy() {
  case $deployMethod in
  tar)
    if [[ -n $releaseChannel ]]; then
      echo "Downloading release from channel: $releaseChannel"
      rm -f "$SOLANA_ROOT"/solana-release.tar.bz2
      declare updateDownloadUrl=https://release.solana.com/"$releaseChannel"/solana-release-x86_64-unknown-linux-gnu.tar.bz2
      (
        set -x
        curl -L -I "$updateDownloadUrl"
        curl -L --retry 5 --retry-delay 2 --retry-connrefused \
          -o "$SOLANA_ROOT"/solana-release.tar.bz2 "$updateDownloadUrl"
      )
      tarballFilename="$SOLANA_ROOT"/solana-release.tar.bz2
    fi
    (
      set -x
      rm -rf "$SOLANA_ROOT"/solana-release
      cd "$SOLANA_ROOT"; tar jfxv "$tarballFilename"
      cat "$SOLANA_ROOT"/solana-release/version.yml
    )
    ;;
  local)
    if $doBuild; then
      build
    else
      echo "Build skipped due to --no-build"
    fi
    ;;
  skip)
    ;;
  *)
    usage "Internal error: invalid deployMethod: $deployMethod"
    ;;
  esac

  if [[ -n $deployIfNewer ]]; then
    if [[ $deployMethod != tar ]]; then
      echo "Error: --deploy-if-newer only supported for tar deployments"
      exit 1
    fi

    echo "Fetching current software version"
    (
      set -x
      rsync -vPrc -e "ssh ${sshOptions[*]}" "${validatorIpList[0]}":~/version.yml current-version.yml
    )
    cat current-version.yml
    if ! diff -q current-version.yml "$SOLANA_ROOT"/solana-release/version.yml; then
      echo "Cluster software version is old.  Update required"
    else
      echo "Cluster software version is current.  No update required"
      exit 0
    fi
  fi
  echo "done preparedeploy()"
}

remoteHomeDir() {
  declare deploymentName=$1
  declare remoteHome
  remoteHome="$(kubectl exec $deploymentName -n $namespace  -- echo \$HOME)"
  echo "$remoteHome"
}

startCommon() {
  # declare ipAddress=$1
  declare deploymentName=$1
  declare remoteHome
  remoteHome=$(remoteHomeDir "$ipAddress")
  local remoteSolanaHome="${remoteHome}/solana"
  local remoteCargoBin="${remoteHome}/.cargo/bin"
  test -d "$SOLANA_ROOT"
  if $skipSetup; then
    # shellcheck disable=SC2029
    ssh "${sshOptions[@]}" "$ipAddress" "
      set -x;
      mkdir -p $remoteSolanaHome/config;
      rm -rf ~/config;
      mv $remoteSolanaHome/config ~;
      rm -rf $remoteSolanaHome;
      mkdir -p $remoteSolanaHome $remoteCargoBin;
      mv ~/config $remoteSolanaHome/
    "
  else
    # shellcheck disable=SC2029
    kubectl exec $deploymentName -n $namespace -- sh -c "rm -rf $remoteSolanaHome && mkdir -p $remoteCargoBin"
    # ssh "${sshOptions[@]}" "$ipAddress" "
    #   set -x;
    #   rm -rf $remoteSolanaHome;
    #   mkdir -p $remoteCargoBin
    # "
  fi
  [[ -z "$externalNodeSshKey" ]] || ssh-copy-id -f -i "$externalNodeSshKey" "${sshOptions[@]}" "solana@$ipAddress"
  syncScripts "$deploymentName"
}

# TODO this could be done in Dockerfile on build?
# or we could do this after we boot up and then we just 
syncScripts() {
  echo "rsyncing scripts... to $deploymentName"
  declare deploymentName=$1
  declare remoteHome
  remoteHome=$(remoteHomeDir "$deploymentName")
  local remoteSolanaHome="${remoteHome}/solana"

  # kubectl cp "$SOLANA_ROOT"/{fetch-perf-libs.sh,fetch-spl.sh,scripts,net,multinode-demo} \
  # "$namespace"/"$deploymentName":/"$remoteSolanaHome"/ > /dev/null

  # rsync -vPrc -e "ssh ${sshOptions[*]}" \
  #   --exclude 'net/log*' \
  #   "$SOLANA_ROOT"/{fetch-perf-libs.sh,fetch-spl.sh,scripts,net,multinode-demo} \
  #   "$ipAddress":"$remoteSolanaHome"/ > /dev/null
}


startBootstrapLeader() {
  # declare ipAddress=$1
  # declare nodeIndex="$2"
  declare logFile="$1"
  echo "--- Starting bootstrap validator"
  echo "start log: $logFile"

  BOOTSTRAP_VALIDATOR_POD=$(kubectl get pods -n $namespace --selector=app.kubernetes.io/name=solana-bootstrap-validator -o jsonpath='{.items[*].metadata.name}')

  (
    set -x
    startCommon $BOOTSTRAP_VALIDATOR_POD
  )

}

deploy() {
  initLogDir

  echo "Deployment started at $(date)"
  $metricsWriteDatapoint "testnet-deploy net-start-begin=1"

  startBootstrapLeader "$netLogDir/bootstrap-validator.log"
}


command=$1
[[ -n $command ]] || usage
shift

shortArgs=()
while [[ -n $1 ]]; do
  shortArgs+=("$1")
  shift
done

while getopts "h?p:n:c:u" opt "${shortArgs[@]}"; do
  case $opt in
  h | \?)
    usage
    ;;
  p)
    [[ ${OPTARG//[^A-Za-z0-9-]/} == "$OPTARG" ]] || usage "Invalid namespace: \"$OPTARG\", alphanumeric only"
    namespace=$OPTARG
    ;;
  n)
    numValidatorsRequested=$OPTARG
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done

echo \
"namespace: $namespace 
validator count: $additionalValidatorCount"

case $command in 
start)
  prepareDeploy
  # stop
  deploy
  ;;
delete)
  stop
  ;;
*)
  echo "Internal error: Unknown command: $command"
  usage
  exit 1
esac