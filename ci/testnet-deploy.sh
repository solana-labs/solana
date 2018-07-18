#!/bin/bash -e
#
# Deploys the Solana software running on the testnet full nodes
#
# This script must be run by a user/machine that has successfully authenticated
# with GCP and has sufficient permission.
#

cd "$(dirname "$0")/.."

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  echo Error: SOLANA_METRICS_CONFIG environment variable is unset
  exit 1
fi

# Default to edge channel.  To select the beta channel:
#   export SOLANA_SNAP_CHANNEL=beta
if [[ -z $SOLANA_SNAP_CHANNEL ]]; then
  SOLANA_SNAP_CHANNEL=edge
fi

case $SOLANA_SNAP_CHANNEL in
edge)
  publicUrl=master.testnet.solana.com
  publicIp=$(dig +short $publicUrl | head -n1)
  ;;
beta)
  publicUrl=testnet.solana.com
  publicIp=#  # Use default value
  ;;
*)
  echo Error: Unknown SOLANA_SNAP_CHANNEL=$SOLANA_SNAP_CHANNEL
  exit 1
  ;;
esac

resourcePrefix=${publicUrl//./-}
vmlist=("$resourcePrefix":us-west1-b) # Leader is hard coded as the first entry
validatorNamePrefix=$resourcePrefix-validator-

echo "--- Available validators for $publicUrl"
filter="name~^$validatorNamePrefix"
gcloud compute instances list --filter="$filter"
while read -r vmName vmZone status; do
  if [[ $status != RUNNING ]]; then
    echo "Warning: $vmName is not RUNNING, ignoring it."
    continue
  fi
  vmlist+=("$vmName:$vmZone")
done < <(gcloud compute instances list --filter="$filter" --format 'value(name,zone,status)')

wait_for_node() {
  declare pid=$1

  declare ok=true
  wait "$pid" || ok=false
  cat "log-$pid.txt"
  if ! $ok; then
    echo ^^^ +++
    exit 1
  fi
}


echo "--- Refreshing leader for $publicUrl"
leader=true
pids=()
count=1
for info in "${vmlist[@]}"; do
  nodePosition="($count/${#vmlist[*]})"

  vmName=${info%:*}
  vmZone=${info#*:}
  echo "Starting refresh for $vmName $nodePosition"

  (
    SECONDS=0
    echo "--- $vmName in zone $vmZone $nodePosition"
    commonNodeConfig="\
      rust-log=$RUST_LOG \
      default-metrics-rate=$SOLANA_DEFAULT_METRICS_RATE \
      metrics-config=$SOLANA_METRICS_CONFIG \
    "
    if $leader; then
      nodeConfig="mode=leader+drone $commonNodeConfig"
      if [[ -n $SOLANA_CUDA ]]; then
        nodeConfig="$nodeConfig enable-cuda=1"
      fi
    else
      nodeConfig="mode=validator leader-address=$publicIp $commonNodeConfig"
    fi

    set -x
    gcloud compute ssh "$vmName" --zone "$vmZone" \
      --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -t" \
      --command="\
        set -ex; \
        logmarker='solana deploy $(date)/$RANDOM'; \
        sudo snap remove solana; \
        logger \$logmarker; \
        sudo snap install solana --$SOLANA_SNAP_CHANNEL --devmode; \
        sudo snap set solana $nodeConfig; \
        snap info solana; \
        echo Slight delay to get more syslog output; \
        sleep 2; \
        sudo grep -Pzo \"\$logmarker(.|\\n)*\" /var/log/syslog \
      "
    echo "Succeeded in ${SECONDS} seconds"
  ) > "log-$vmName.txt" 2>&1 &
  pid=$!
  # Rename log file so it can be discovered later by $pid
  mv "log-$vmName.txt" "log-$pid.txt"

  if $leader; then
    echo Waiting for leader...
    # Wait for the leader to initialize before starting the validators
    # TODO: Remove this limitation eventually.
    wait_for_node "$pid"

    echo "--- Refreshing validators"
  else
    #  Slow down deployment to ~20 machines a minute to avoid triggering GCP login
    #  quota limits (each |ssh| counts as a login)
    sleep 3

    pids+=("$pid")
  fi
  leader=false
  count=$((count + 1))
done

echo --- Waiting for validators
for pid in "${pids[@]}"; do
  wait_for_node "$pid"
done

echo "--- $publicUrl sanity test"
USE_SNAP=1 ci/testnet-sanity.sh $publicUrl

exit 0
