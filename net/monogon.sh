#!/usr/bin/env bash
set -e

here=$(dirname "$0")
# shellcheck source=net/common.sh
source "$here"/common.sh

namespace=testnet-dev-${USER//[^A-Za-z0-9]/}
additionalValidatorCount=2
clientNodeCount=0
blockstreamer=false

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|delete] [common options]

Manage monogon instances
 create - create a new testnet (implies 'config')
 config - configure the testnet and write a config file describing it
 delete - delete the testnet

 common options:
   -p [namespace]      - Optional namespace for deployment
   -n [number]         - Number of additional validators (default: $additionalValidatorCount)
   -c [number]         - Number of client nodes (default: $clientNodeCount)
   -u                  - Include a Blockstreamer (default: $blockstreamer)
EOF
  exit $exitcode
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
    echo "Creating Namespace $namespace..."
    kubectl create ns "$namespace" 

    echo "Deploying bootstrap deployment/service"
    deployBootstrapValidatorDeployments

    echo "Deploying validator deployments/services"
    deployValidatorDeployments
}

deployBootstrapValidatorDeployments() {
    kubectl apply -f "$here/k8s-cluster/deployments/bootstrap-validator.yaml" --namespace=$namespace
    
    ATTEMPTS=0
    ROLLOUT_STATUS_COMMAND="kubectl rollout status deployment/solana-bootstrap-validator-deployment -n $namespace"
    until $ROLLOUT_STATUS_COMMAND || [ $ATTEMPTS -eq 10 ]; do
        $ROLLOUT_STATUS_COMMAND
        ATTEMPTS=$((attempts + 1))
        sleep 2
    done
}

deployValidatorDeployments() {
    

}





command=$1
[[ -n $command ]] || usage
shift
[[ $command = create || $command = create || $command = delete ]] ||
  usage "Invalid command: $command"

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
    additionalValidatorCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
    ;;
  u)
    blockstreamer=true
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done

echo \
"namespace: $namespace 
validator count: $additionalValidatorCount
client count: $clientNodeCount 
blockstreamer: $blockstreamer"

case $command in 
create)
#   prepareDeploy
  stop
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