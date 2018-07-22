#!/bin/bash -e
#
# Deploys the Solana software running on the testnet full nodes
#
# This script must be run by a user/machine that has successfully authenticated
# with GCP and has sufficient permission.
#
cd "$(dirname "$0")/.."

# TODO: Switch over to rolling updates
ROLLING_UPDATE=false
#ROLLING_UPDATE=true

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  echo Error: SOLANA_METRICS_CONFIG environment variable is unset
  exit 1
fi

# Default to edge channel.  To select the beta channel:
#   export SOLANA_SNAP_CHANNEL=beta
if [[ -z $SOLANA_SNAP_CHANNEL ]]; then
  SOLANA_SNAP_CHANNEL=edge
fi

# Select default network URL based on SOLANA_SNAP_CHANNEL if SOLANA_NET_URL is
# unspecified
if [[ -z $SOLANA_NET_URL ]]; then
  case $SOLANA_SNAP_CHANNEL in
  edge)
    SOLANA_NET_URL=master.testnet.solana.com
    ;;
  beta)
    SOLANA_NET_URL=testnet.solana.com
    ;;
  *)
    echo Error: Unknown SOLANA_SNAP_CHANNEL=$SOLANA_SNAP_CHANNEL
    exit 1
    ;;
  esac
fi

echo "+++ Configuration"
publicUrl="$SOLANA_NET_URL"
if [[ $publicUrl = testnet.solana.com ]]; then
  publicIp="" # Use default value
else
  publicIp=$(dig +short $publicUrl | head -n1)
fi

echo "Network entrypoint URL: $publicUrl ($publicIp)"
echo "Snap channel: $SOLANA_SNAP_CHANNEL"

leaderName=${publicUrl//./-}
vmlist=()

findVms() {
  declare filter="$1"
  gcloud compute instances list --filter="$filter"
  while read -r vmName vmZone status; do
    if [[ $status != RUNNING ]]; then
      echo "Warning: $vmName is not RUNNING, ignoring it."
      continue
    fi
    vmlist+=("$vmName:$vmZone")
  done < <(gcloud compute instances list --filter="$filter" --format 'value(name,zone,status)')
}

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

echo "Leader node:"
findVms "name=$leaderName"
[[ ${#vmlist[@]} = 1 ]] || {
  echo "Unable to find $leaderName"
  exit 1
}

echo "Client node:"
findVms "name=$leaderName-client"
clientVm=
if [[ ${#vmlist[@]} = 2 ]]; then
  clientVm=${vmlist[1]}
  unset 'vmlist[1]'
fi

echo "Validator nodes:"
findVms "name~^$leaderName-validator-"

if ! $ROLLING_UPDATE; then
  count=1
  for info in "${vmlist[@]}"; do
    nodePosition="($count/${#vmlist[*]})"
    vmName=${info%:*}
    vmZone=${info#*:}
    echo "--- Shutting down $vmName in zone $vmZone $nodePosition"
    gcloud compute ssh "$vmName" --zone "$vmZone" \
      --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
      --command="sudo snap remove solana" &

    if [[ $((count % 5)) = 0 ]]; then
      #  Slow down deployment to avoid triggering GCP login
      #  quota limits (each |ssh| counts as a login)
      sleep 3
    fi

    count=$((count + 1))
  done

  wait
fi

# Add "network stopping" datapoint
netName=${SOLANA_NET_URL%testnet.solana.com}
netName=${netName:0:8}
ci/metrics_write_datapoint.sh "testnet-deploy,name=\"$netName\" stop=1"

client_run() {
  declare message=$1
  declare cmd=$2
  [[ -n $clientVm ]] || return 0;
  vmName=${clientVm%:*}
  vmZone=${clientVm#*:}
  echo "--- $message $vmName in zone $vmZone"
  gcloud compute ssh "$vmName" --zone "$vmZone" \
    --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
    --command="$cmd"
}

client_run \
  "Shutting down" \
  "\
    set -x;
    tmux list-sessions; \
    tmux capture-pane -t solana -p; \
    tmux kill-session -t solana; \
    sudo snap remove solana; \
  "

echo "--- Refreshing leader"
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
(
  set -x
  USE_SNAP=1 ci/testnet-sanity.sh $publicUrl ${#vmlist[@]}
)

client_run \
  "Starting client on " \
  "\
    set -x;
    sudo snap install solana --$SOLANA_SNAP_CHANNEL --devmode; \
    snap info solana; \
    tmux new -s solana -d \" \
        /snap/bin/solana.bench-tps $SOLANA_NET_URL ${#vmlist[@]} --loop -s 600 2>&1 | tee /tmp/solana.log; \
        echo Error: bench-tps should never exit; \
        bash \
      \"; \
    sleep 2; \
    tmux capture-pane -t solana -p -S -100; \
    tail /tmp/solana.log; \
  "

# Add "network started" datapoint
ci/metrics_write_datapoint.sh "testnet-deploy,name=\"$netName\" start=1"

exit 0
