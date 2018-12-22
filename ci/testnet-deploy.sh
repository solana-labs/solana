#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/..

zone=
bootstrapFullNodeAddress=
bootstrapFullNodeMachineType=
clientNodeCount=0
additionalFullNodeCount=10
publicNetwork=false
snapChannel=edge
tarChannelOrTag=edge
delete=false
enableGpu=false
leaderRotation=true
useTarReleaseChannel=false

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [name] [cloud] [zone] [options...]

Deploys a CD testnet

  name  - name of the network
  cloud - cloud provider to use (gce, ec2)
  zone  - cloud provider zone to deploy the network into

  options:
   -s edge|beta|stable  - Deploy the specified Snap release channel
                          (default: $snapChannel)
   -t edge|beta|stable|vX.Y.Z  - Deploy the latest tarball release for the
                                 specified release channel (edge|beta|stable) or release tag
                                 (vX.Y.Z)
                                 (default: $tarChannelOrTag)
   -n [number]          - Number of additional full nodes (default: $additionalFullNodeCount)
   -c [number]          - Number of client bencher nodes (default: $clientNodeCount)
   -P                   - Use public network IP addresses (default: $publicNetwork)
   -G                   - Enable GPU, and set count/type of GPUs to use (e.g n1-standard-16 --accelerator count=4,type=nvidia-tesla-k80)
   -g                   - Enable GPU (default: $enableGpu)
   -b                   - Disable leader rotation
   -a [address]         - Set the bootstrap fullnode's external IP address to this GCE address
   -d                   - Delete the network

   Note: the SOLANA_METRICS_CONFIG environment variable is used to configure
         metrics
EOF
  exit $exitcode
}

netName=$1
cloudProvider=$2
zone=$3
[[ -n $netName ]] || usage
[[ -n $cloudProvider ]] || usage "Cloud provider not specified"
[[ -n $zone ]] || usage "Zone not specified"
shift 3

while getopts "h?p:Pn:c:s:t:gG:a:db" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  P)
    publicNetwork=true
    ;;
  n)
    additionalFullNodeCount=$OPTARG
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
  t)
    case $OPTARG in
    edge|beta|stable|v*)
      tarChannelOrTag=$OPTARG
      useTarReleaseChannel=true
      ;;
    *)
      usage "Invalid release channel: $OPTARG"
      ;;
    esac
    ;;
  b)
    leaderRotation=false
    ;;
  g)
    enableGpu=true
    ;;
  G)
    enableGpu=true
    bootstrapFullNodeMachineType=$OPTARG
    ;;
  a)
    bootstrapFullNodeAddress=$OPTARG
    ;;
  d)
    delete=true
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done


create_args=(
  -a "$bootstrapFullNodeAddress"
  -c "$clientNodeCount"
  -n "$additionalFullNodeCount"
  -p "$netName"
  -z "$zone"
)

if $enableGpu; then
  if [[ -z $bootstrapFullNodeMachineType ]]; then
    create_args+=(-g)
  else
    create_args+=(-G "$bootstrapFullNodeMachineType")
  fi
fi

if ! $leaderRotation; then
  create_args+=(-b)
fi

if $publicNetwork; then
  create_args+=(-P)
fi

shutdown() {
  exitcode=$?

  set +e
  echo "--- Upload artifacts"
  for logfile in net/log/*; do
    if [[ -f $logfile ]]; then
      upload-ci-artifact "$logfile"
      tail "$logfile"
    fi
  done

  exit $exitcode
}

trap shutdown EXIT INT

set -x

echo "--- $cloudProvider.sh delete"
time net/"$cloudProvider".sh delete -z "$zone" -p "$netName"
if $delete; then
  exit 0
fi

echo "--- $cloudProvider.sh create"
time net/"$cloudProvider".sh create "${create_args[@]}"
net/init-metrics.sh -e

echo --- net.sh start
maybeRejectExtraNodes=
if ! $publicNetwork; then
  maybeRejectExtraNodes="-o rejectExtraNodes"
fi
maybeNoValidatorSanity=
if [[ -n $NO_VALIDATOR_SANITY ]]; then
  maybeNoValidatorSanity="-o noValidatorSanity"
fi
maybeNoLedgerVerify=
if [[ -n $NO_LEDGER_VERIFY ]]; then
  maybeNoLedgerVerify="-o noLedgerVerify"
fi
# shellcheck disable=SC2086 # Don't want to double quote maybeRejectExtraNodes
if $useTarReleaseChannel; then
  time net/net.sh start -t "$tarChannelOrTag" $maybeRejectExtraNodes $maybeNoValidatorSanity $maybeNoLedgerVerify
else
  time net/net.sh start -s "$snapChannel" $maybeRejectExtraNodes $maybeNoValidatorSanity $maybeNoLedgerVerify
fi
exit 0
