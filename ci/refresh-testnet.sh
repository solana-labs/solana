#!/bin/bash
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

# Default to --edge channel.  To select the beta channel:
#   export SOLANA_METRICS_CONFIG=--beta
if [[ -z $SOLANA_SNAP_CHANNEL ]]; then
  SOLANA_SNAP_CHANNEL=--edge
fi

vmlist=(testnet-solana-com:us-west1-b) # Leader is hard coded as the first entry

echo "--- Available validators"
gcloud compute instances list --filter="labels.testnet-mode=validator"
while read -r vmName vmZone status; do
  if [[ $status != RUNNING ]]; then
    echo "Warning: $vmName is not RUNNING, ignoring it."
    continue
  fi
  vmlist+=("$vmName:$vmZone")
done < <(gcloud compute instances list --filter="labels.testnet-mode=validator" --format 'value(name,zone,status)')


echo "--- Refreshing"
mode=leader+drone
for info in "${vmlist[@]}"; do
  vmName=${info%:*}
  vmZone=${info#*:}
  echo "Starting refresh for $vmName"

  (
    echo "--- Processing $vmName in zone $vmZone as $mode"
    cat > "autogen-refresh-$vmName.sh" <<EOF
      set -x
      sudo snap remove solana
      sudo snap install solana $SOLANA_SNAP_CHANNEL --devmode
      sudo snap set solana mode=$mode metrics-config=$SOLANA_METRICS_CONFIG
      snap info solana
      sudo snap logs solana -n200
EOF
    set -x
    gcloud compute scp --zone "$vmZone" "autogen-refresh-$vmName.sh" "$vmName":
    gcloud compute ssh "$vmName" --zone "$vmZone" \
      --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -t" \
      --command="bash ./autogen-refresh-$vmName.sh"
  ) > "log-$vmName.txt" 2>&1 &
  mode=validator
done

echo "Waiting..."
wait

for info in "${vmlist[@]}"; do
  vmName=${info%:*}
  cat "log-$vmName.txt"
done

echo "--- done"
exit 0
