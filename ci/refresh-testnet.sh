#!/bin/bash -e
#
# Refreshes the Solana software running on the Testnet full nodes
#
# This script must be run by a user/machine that has successfully authenticated
# with GCP and has sufficient permission.
#

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
  resourcePrefix=master-testnet-solana-com
  ;;
beta)
  resourcePrefix=testnet-solana-com
  ;;
*)
  echo Error: Unknown SOLANA_SNAP_CHANNEL=$SOLANA_SNAP_CHANNEL
  exit 1
  ;;
esac

publicUrl=${resourcePrefix//-/.}
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

echo "--- Refreshing leader for $publicUrl"
leader=true
logfiles=()
for info in "${vmlist[@]}"; do
  vmName=${info%:*}
  vmZone=${info#*:}
  echo "Starting refresh for $vmName"

  (
    SECONDS=0
    echo "--- $vmName in zone $vmZone"
    if $leader; then
      nodeConfig="mode=leader+drone enable-cuda=1 metrics-config=$SOLANA_METRICS_CONFIG"
    else
      nodeConfig="mode=validator metrics-config=$SOLANA_METRICS_CONFIG"
    fi
    cat > "autogen-refresh-$vmName.sh" <<EOF
      set -x
      sudo snap remove solana
      sudo snap install solana --$SOLANA_SNAP_CHANNEL --devmode
      sudo snap set solana $nodeConfig
      snap info solana
      sudo snap logs solana -n200
EOF

    set -x
    gcloud compute scp --zone "$vmZone" "autogen-refresh-$vmName.sh" "$vmName":
    gcloud compute ssh "$vmName" --zone "$vmZone" \
      --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -t" \
      --command="bash ./autogen-refresh-$vmName.sh"
    echo "Succeeded in ${SECONDS} seconds"
  ) > "log-$vmName.txt" 2>&1 &

  if $leader; then
    echo Waiting for leader...
    # Wait for the leader to initialize before starting the validators
    # TODO: Remove this limitation eventually.
    wait

    cat "log-$vmName.txt"
    echo "--- Refreshing validators"
  else
    #  Slow down deployment to ~30 machines a minute to avoid triggering GCP login
    #  quota limits (the previous |scp| and |ssh| each count as a login)
    sleep 2

    logfiles+=("log-$vmName.txt")
  fi
  leader=false
done

echo --- Waiting for validators
wait

for log in "${logfiles[@]}"; do
  cat "$log"
done

echo "--- $publicUrl sanity test"
USE_SNAP=1 ./multinode-demo/test/wallet-sanity.sh $publicUrl

exit 0
