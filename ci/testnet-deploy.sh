#!/bin/bash -e
#
# Deploys the Solana software running on the testnet full nodes
#
# This script must be run by a user/machine that has successfully authenticated
# with GCP and has sufficient permission.
#
here=$(dirname "$0")

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

# Figure installation command
SNAP_INSTALL_CMD="sudo snap install solana --$SOLANA_SNAP_CHANNEL --devmode"
LOCAL_SNAP=$1
if [[ -n $LOCAL_SNAP ]]; then
  if [[ ! -f $LOCAL_SNAP ]]; then
    echo "Error: $LOCAL_SNAP is not a file"
    exit 1
  fi
  SNAP_INSTALL_CMD="sudo snap install ~/solana_local.snap --devmode --dangerous"
fi
SNAP_INSTALL_CMD="sudo snap remove solana; $SNAP_INSTALL_CMD"

# `export SKIP_INSTALL=1` to reset the network without reinstalling the snap
if [[ -n $SKIP_INSTALL ]]; then
  SNAP_INSTALL_CMD="echo Install skipped"
fi

echo "+++ Configuration"
publicUrl="$SOLANA_NET_URL"
if [[ $publicUrl = testnet.solana.com ]]; then
  publicIp="" # Use default value
else
  publicIp=$(dig +short $publicUrl | head -n1)
fi

echo "Network entry point URL: $publicUrl ($publicIp)"
echo "Snap channel: $SOLANA_SNAP_CHANNEL"
echo "Install command: $SNAP_INSTALL_CMD"
[[ -z $LOCAL_SNAP ]] || echo "Local snap: $LOCAL_SNAP"

leaderName=${publicUrl//./-}

vmlist=() # Each array element is the triple "class:vmName:vmZone"

#
# vm_foreach [cmd] [extra args to cmd]
# where
#   cmd   - the command to execute on each VM
#           The command will receive three fixed arguments, followed by any
#           additionl arguments supplied to vm_foreach:
#               vmName - GCP name of the VM
#               vmZone - The GCP zone the VM is located in
#               vmClass - The 'class' of this VM
#               count  - Monotonically increasing count for each
#                        invocation of cmd, starting at 1
#               ...    - Extra args to cmd..
#
#
vm_foreach() {
  declare cmd=$1
  shift

  declare count=1
  for info in "${vmlist[@]}"; do
    declare vmClass vmName vmZone
    IFS=: read -r vmClass vmName vmZone < <(echo "$info")

    eval "$cmd" "$vmName" "$vmZone" "$vmClass" "$count" "$@"
    count=$((count + 1))
  done
}

#
# vm_foreach_in_class [class] [cmd]
# where
#   class - the desired VM class to operate on
#   cmd   - the command to execute on each VM in the desired class.
#           The command will receive three arguments:
#               vmName - GCP name of the VM
#               vmZone - The GCP zone the VM is located in
#               count  - Monotonically increasing count for each
#                        invocation of cmd, starting at 1
#
#
_run_cmd_if_class() {
  declare vmName=$1
  declare vmZone=$2
  declare vmClass=$3
  declare count=$4
  declare class=$5
  declare cmd=$6
  if [[ $class = "$vmClass" ]]; then
    eval "$cmd" "$vmName" "$vmZone" "$count"
  fi
}

vm_foreach_in_class() {
  declare class=$1
  declare cmd=$2
  vm_foreach _run_cmd_if_class "$1" "$2"
}

#
# Load all VMs matching the specified filter and tag them with the specified
# class into the `vmlist` array.
findVms() {
  declare class="$1"
  declare filter="$2"
  gcloud compute instances list --filter="$filter"
  while read -r vmName vmZone status; do
    if [[ $status != RUNNING ]]; then
      echo "Warning: $vmName is not RUNNING, ignoring it."
      continue
    fi
    vmlist+=("$class:$vmName:$vmZone")
  done < <(gcloud compute instances list --filter="$filter" --format 'value(name,zone,status)')
}

echo "Leader node:"
findVms leader "name=$leaderName"
[[ ${#vmlist[@]} = 1 ]] || {
  echo "Unable to find $leaderName"
  exit 1
}

echo "Client node(s):"
findVms client "name~^$leaderName-client"

echo "Validator nodes:"
findVms validator "name~^$leaderName-validator-"

fullnode_count=0
inc_fullnode_count() {
  fullnode_count=$((fullnode_count + 1))
}
vm_foreach_in_class leader inc_fullnode_count
vm_foreach_in_class validator inc_fullnode_count

# Add "network stopping" datapoint
netName=${SOLANA_NET_URL/.*/}
"$here"/metrics_write_datapoint.sh "testnet-deploy,name=$netName stop=1"

gcp_vm_exec() {
  declare vmName=$1
  declare vmZone=$2
  declare message=$3
  declare cmd=$4

  echo "--- $message $vmName in zone $vmZone"
  gcloud compute ssh "$vmName" --zone "$vmZone" \
    --ssh-flag="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
    --command="$cmd"
}

gcp_login_quota_workaround() {
  declare count=$1

  if [[ $((count % 5)) = 0 ]]; then
    #  Slow down deployment to avoid triggering GCP login
    #  quota limits (each |ssh| counts as a login)
    sleep 3
  fi
}

client_start() {
  declare vmName=$1
  declare vmZone=$2
  declare count=$3

  gcp_vm_exec "$vmName" "$vmZone" \
    "Starting client $count:" \
    "\
      set -x;
      snap info solana; \
      sudo snap get solana; \
      threadCount=\$(nproc); \
      if [[ \$threadCount -gt 4 ]]; then threadCount=4; fi; \
      tmux kill-session -t solana; \
      tmux new -s solana -d \" \
          set -x;
          /snap/bin/solana.bench-tps $SOLANA_NET_URL $fullnode_count --loop -s 600 --sustained -t \$threadCount 2>&1 | tee /tmp/solana.log; \
          echo 'https://metrics.solana.com:8086/write?db=${INFLUX_DATABASE}&u=${INFLUX_USERNAME}&p=${INFLUX_PASSWORD}' \
            | xargs curl -XPOST --data-binary 'testnet-deploy,name=$netName clientexit=1'; \
          echo Error: bench-tps should never exit | tee -a /tmp/solana.log; \
          bash \
        \"; \
      sleep 2; \
      tmux capture-pane -t solana -p -S -100; \
      tail /tmp/solana.log; \
  "
}

client_stop() {
  declare vmName=$1
  declare vmZone=$2
  declare count=$3

  (
    SECONDS=0
    gcp_vm_exec "$vmName" "$vmZone" \
      "Stopping client $count:" \
      "\
        set -x;
        tmux list-sessions; \
        tmux capture-pane -t solana -p; \
        tmux kill-session -t solana; \
        $SNAP_INSTALL_CMD; \
        sudo snap set solana metrics-config=$SOLANA_METRICS_CONFIG \
          rust-log=$RUST_LOG \
          default-metrics-rate=$SOLANA_DEFAULT_METRICS_RATE \
        ; \
      "
    echo "Client stopped in ${SECONDS} seconds"
  ) > "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  while [[ ! -f "log-$vmName.txt" ]]; do
    sleep 1
  done
  mv "log-$vmName.txt" "log-$pid.txt"
  pids+=("$pid")
}

fullnode_start() {
  declare class=$1
  declare vmName=$2
  declare vmZone=$3
  declare count=$4

  (
    SECONDS=0
    commonNodeConfig="\
      rust-log=$RUST_LOG \
      default-metrics-rate=$SOLANA_DEFAULT_METRICS_RATE \
      metrics-config=$SOLANA_METRICS_CONFIG \
    "
    if [[ $class = leader ]]; then
      nodeConfig="mode=leader+drone $commonNodeConfig"
      if [[ -n $SOLANA_CUDA ]]; then
        nodeConfig="$nodeConfig enable-cuda=1"
      fi
    else
      nodeConfig="mode=validator leader-address=$publicIp $commonNodeConfig"
    fi

    gcp_vm_exec "$vmName" "$vmZone" "Starting $class $count:" \
      "\
        set -ex; \
        logmarker='solana deploy $(date)/$RANDOM'; \
        logger \"\$logmarker\"; \
        $SNAP_INSTALL_CMD; \
        sudo snap set solana $nodeConfig; \
        snap info solana; \
        sudo snap get solana; \
        echo Slight delay to get more syslog output; \
        sleep 2; \
        sudo grep -Pzo \"\$logmarker(.|\\n)*\" /var/log/syslog \
      "
    echo "Succeeded in ${SECONDS} seconds"
  ) > "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  while [[ ! -f "log-$vmName.txt" ]]; do
    sleep 1
  done
  mv "log-$vmName.txt" "log-$pid.txt"

  gcp_login_quota_workaround "$count"
  pids+=("$pid")
}

leader_start() {
  fullnode_start leader "$@"
}

validator_start() {
  fullnode_start validator "$@"
}

fullnode_stop() {
  declare vmName=$1
  declare vmZone=$2
  declare count=$3

  (
    SECONDS=0
    gcp_vm_exec "$vmName" "$vmZone" "Shutting down" "\
      if snap list solana; then \
        sudo snap set solana mode=; \
      fi"
    echo "Succeeded in ${SECONDS} seconds"
  ) > "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  while [[ ! -f "log-$vmName.txt" ]]; do
    sleep 1
  done
  mv "log-$vmName.txt" "log-$pid.txt"

  gcp_login_quota_workaround "$count"
  pids+=("$pid")
}

wait_for_pids() {
  echo "--- Waiting for $*"
  for pid in "${pids[@]}"; do
    declare ok=true
    wait "$pid" || ok=false
    cat "log-$pid.txt"
    if ! $ok; then
      echo ^^^ +++
      exit 1
    fi
    rm "log-$pid.txt"
  done
}

if [[ -n $LOCAL_SNAP ]]; then
  echo "--- Transferring $LOCAL_SNAP to node(s)"

  transfer_local_snap() {
    declare vmName=$1
    declare vmZone=$2
    declare vmClass=$3
    declare count=$4

    echo "--- $vmName in zone $vmZone ($count)"
    SECONDS=0
    gcloud compute scp --zone "$vmZone" "$LOCAL_SNAP" "$vmName":solana_local.snap
    echo "Succeeded in ${SECONDS} seconds"
  }
  vm_foreach transfer_local_snap
fi

pids=()
echo "--- Stopping client node(s)"
vm_foreach_in_class client client_stop
client_stop_pids=("${pids[@]}")

if ! $ROLLING_UPDATE; then
  pids=()
  echo "--- Shutting down all full nodes"
  vm_foreach_in_class leader fullnode_stop
  vm_foreach_in_class validator fullnode_stop
  wait_for_pids fullnode shutdown
fi

pids=()
echo --- Starting leader node
vm_foreach_in_class leader leader_start
wait_for_pids leader

pids=()
echo --- Starting validator nodes
vm_foreach_in_class validator validator_start
wait_for_pids validators

echo "--- $publicUrl sanity test"
if [[ -z $CI ]]; then
  # TODO: ssh into a node and run testnet-sanity.sh there.  It's not safe to
  #       assume the correct Snap is installed on the current non-CI machine
  echo Skipped for non-CI deploy
else
  (
    set -x
    USE_SNAP=1 ci/testnet-sanity.sh $publicUrl $fullnode_count
  )
fi

pids=("${client_stop_pids[@]}")
wait_for_pids client shutdown
vm_foreach_in_class client client_start

# Add "network started" datapoint
"$here"/metrics_write_datapoint.sh "testnet-deploy,name=$netName start=1"

exit 0
