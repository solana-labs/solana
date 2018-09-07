#!/bin/bash -e

cd "$(dirname "$0")"/..

zone=
leaderAddress=
clientNodeCount=0
validatorNodeCount=10
publicNetwork=false
snapChannel=edge
delete=false

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [name] [zone] [options...]

Deploys a CD testnet

  name  - name of the network
  zone   - GCE to deploy the network into

  options:
   -s edge|beta|stable  - Deploy the specified Snap release channel
                          (default: $snapChannel)
   -n [number]          - Number of validator nodes (default: $validatorNodeCount)
   -c [number]          - Number of client nodes (default: $clientNodeCount)
   -P                   - Use public network IP addresses (default: $publicNetwork)
   -a [address]         - Set the leader node's external IP address to this GCE address
   -d                   - Delete the network

   Note: the SOLANA_METRICS_CONFIG environment variable is used to configure
         metrics
EOF
  exit $exitcode
}

netName=$1
zone=$2
[[ -n $netName ]] || usage
[[ -n $zone ]] || usage "Zone not specified"
shift 2

while getopts "h?p:Pn:c:s:a:d" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  P)
    publicNetwork=true
    ;;
  n)
    validatorNodeCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
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
    leaderAddress=$OPTARG
    ;;
  d)
    delete=true
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done


gce_create_args=(
  -a "$leaderAddress"
  -c "$clientNodeCount"
  -n "$validatorNodeCount"
  -g
  -p "$netName"
  -z "$zone"
)

if $publicNetwork; then
  gce_create_args+=(-P)
fi

set -x

echo --- gce.sh delete
time net/gce.sh delete -p "$netName"
if $delete; then
  exit 0
fi

echo --- gce.sh create
time net/gce.sh create "${gce_create_args[@]}"
net/init-metrics.sh -e

echo --- net.sh start
time net/net.sh start -s "$snapChannel"

exit 0
