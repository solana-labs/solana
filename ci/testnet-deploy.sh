#!/bin/bash -e
#
# Deploys the Solana software running on the testnet full nodes
#
# This script must be run by a user/machine that has successfully authenticated
# with GCP and has sufficient permission.
#
here=$(dirname "$0")
metrics_write_datapoint="$here"/../multinode-demo/metrics_write_datapoint.sh

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

# Select default network URL based on SOLANA_SNAP_CHANNEL if SOLANA_NET_ENTRYPOINT is
# unspecified
if [[ -z $SOLANA_NET_ENTRYPOINT ]]; then
  case $SOLANA_SNAP_CHANNEL in
  edge)
    SOLANA_NET_ENTRYPOINT=master.testnet.solana.com
    unset SOLANA_NET_NAME
    ;;
  beta)
    SOLANA_NET_ENTRYPOINT=testnet.solana.com
    unset SOLANA_NET_NAME
    ;;
  *)
    echo Error: Unknown SOLANA_SNAP_CHANNEL=$SOLANA_SNAP_CHANNEL
    exit 1
    ;;
  esac
fi

if [[ -z $SOLANA_NET_NAME ]]; then
  SOLANA_NET_NAME=${SOLANA_NET_ENTRYPOINT//./-}
fi

: ${SOLANA_NET_NAME:?$SOLANA_NET_ENTRYPOINT}
netBasename=${SOLANA_NET_NAME/-*/}
if [[ $netBasename != testnet ]]; then
  netBasename="testnet-$netBasename"
fi

# Figure installation command
SNAP_INSTALL_CMD="\
  for i in {1..3}; do \
    sudo snap install solana --$SOLANA_SNAP_CHANNEL --devmode && break;
    sleep 1; \
  done \
"
LOCAL_SNAP=$1
if [[ -n $LOCAL_SNAP ]]; then
  if [[ ! -f $LOCAL_SNAP ]]; then
    echo "Error: $LOCAL_SNAP is not a file"
    exit 1
  fi
  SNAP_INSTALL_CMD="sudo snap install ~/solana_local.snap --devmode --dangerous"
fi
SNAP_INSTALL_CMD="sudo snap remove solana; $SNAP_INSTALL_CMD"

EARLYOOM_INSTALL_CMD="\
  wget -O install-earlyoom.sh https://raw.githubusercontent.com/solana-labs/solana/master/ci/install-earlyoom.sh; \
  bash install-earlyoom.sh \
"
SNAP_INSTALL_CMD="$EARLYOOM_INSTALL_CMD; $SNAP_INSTALL_CMD"

# `export SKIP_INSTALL=1` to reset the network without reinstalling the snap
if [[ -n $SKIP_INSTALL ]]; then
  SNAP_INSTALL_CMD="echo Install skipped"
fi

echo "+++ Configuration for $netBasename"
publicUrl="$SOLANA_NET_ENTRYPOINT"
if [[ $publicUrl = testnet.solana.com ]]; then
  publicIp="" # Use default value
else
  publicIp=$(dig +short $publicUrl | head -n1)
fi

echo "Network name: $SOLANA_NET_NAME"
echo "Network entry point URL: $publicUrl ($publicIp)"
echo "Snap channel: $SOLANA_SNAP_CHANNEL"
echo "Install command: $SNAP_INSTALL_CMD"
echo "Setup args: $SOLANA_SETUP_ARGS"
[[ -z $LOCAL_SNAP ]] || echo "Local snap: $LOCAL_SNAP"

vmlist=() # Each array element is formatted as "class:vmName:vmZone:vmPublicIp"

vm_exec() {
  declare vmName=$1
  declare vmZone=$2
  declare vmPublicIp=$3
  declare message=$4
  declare cmd=$5

  echo "--- $message $vmName in zone $vmZone ($vmPublicIp)"
  ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    testnet-deploy@"$vmPublicIp" "$cmd"
}

#
# vm_foreach [cmd] [extra args to cmd]
# where
#   cmd   - the command to execute on each VM
#           The command will receive three fixed arguments, followed by any
#           additionl arguments supplied to vm_foreach:
#               vmName - GCP name of the VM
#               vmZone - The GCP zone the VM is located in
#               vmPublicIp - The public IP address of this VM
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
    declare vmClass vmName vmZone vmPublicIp
    IFS=: read -r vmClass vmName vmZone vmPublicIp < <(echo "$info")

    eval "$cmd" "$vmName" "$vmZone" "$vmPublicIp" "$vmClass" "$count" "$@"
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
#               vmPublicIp - The public IP address of this VM
#               count  - Monotonically increasing count for each
#                        invocation of cmd, starting at 1
#
#
_run_cmd_if_class() {
  declare vmName=$1
  declare vmZone=$2
  declare vmPublicIp=$3
  declare vmClass=$4
  declare count=$5
  declare class=$6
  declare cmd=$7
  if [[ $class = "$vmClass" ]]; then
    eval "$cmd" "$vmName" "$vmZone" "$vmPublicIp" "$count"
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
  while read -r vmName vmZone vmPublicIp status; do
    if [[ $status != RUNNING ]]; then
      echo "Warning: $vmName is not RUNNING, ignoring it."
      continue
    fi
    vmlist+=("$class:$vmName:$vmZone:$vmPublicIp")
  done < <(gcloud compute instances list \
             --filter="$filter" \
             --format 'value(name,zone,networkInterfaces[0].accessConfigs[0].natIP,status)')
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

delete_unreachable_validators() {
  declare vmName=$1
  declare vmZone=$2
  declare vmPublicIp=$3

  touch "log-$vmName.txt"
  (
    SECONDS=0
    if ! vm_exec "$vmName" "$vmZone" "$vmPublicIp" "Checking $vmName" uptime; then
      echo "^^^ +++"

      # Validators are managed by a Compute Engine Instance Group, so deleting
      # one will just cause a new one to be spawned.
      echo "Warning: $vmName is unreachable, deleting it"
      gcloud compute instances delete "$vmName" --zone "$vmZone"
    fi
    echo "validator checked in ${SECONDS} seconds"
  ) >> "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  mv "log-$vmName.txt" "log-$pid.txt"
  pids+=("$pid")
}


echo "Validator nodes (unverified):"
findVms validator "name~^$SOLANA_NET_NAME-validator-"
pids=()
vm_foreach_in_class validator delete_unreachable_validators
wait_for_pids validator sanity check
vmlist=()

echo "Leader node:"
findVms leader "name=$SOLANA_NET_NAME"
[[ ${#vmlist[@]} = 1 ]] || {
  echo "Unable to find $SOLANA_NET_NAME"
  exit 1
}

echo "Client node(s):"
findVms client "name~^$SOLANA_NET_NAME-client"

echo "Validator nodes:"
findVms validator "name~^$SOLANA_NET_NAME-validator-"

fullnode_count=0
inc_fullnode_count() {
  fullnode_count=$((fullnode_count + 1))
}
vm_foreach_in_class leader inc_fullnode_count
vm_foreach_in_class validator inc_fullnode_count

# Add "network stopping" datapoint
$metrics_write_datapoint "testnet-deploy,name=$netBasename stop=1"

client_start() {
  declare vmName=$1
  declare vmZone=$2
  declare vmPublicIp=$3
  declare count=$4

  vm_exec "$vmName" "$vmZone" "$vmPublicIp" \
    "Starting client $count:" \
    "\
      set -x;
      snap info solana; \
      sudo snap get solana; \
      threadCount=\$(nproc); \
      if [[ \$threadCount -gt 4 ]]; then threadCount=4; fi; \
      tmux kill-session -t solana; \
      tmux new -s solana -d \" \
          set -x; \
          sudo rm /tmp/solana.log; \
          /snap/bin/solana.bench-tps $SOLANA_NET_ENTRYPOINT $fullnode_count --loop -s 600 --sustained -t \$threadCount 2>&1 | tee /tmp/solana.log; \
          echo 'https://metrics.solana.com:8086/write?db=${INFLUX_DATABASE}&u=${INFLUX_USERNAME}&p=${INFLUX_PASSWORD}' \
            | xargs curl --max-time 5 -XPOST --data-binary 'testnet-deploy,name=$netBasename clientexit=1'; \
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
  declare vmPublicIp=$3
  declare count=$4

  touch "log-$vmName.txt"
  (
    SECONDS=0
    vm_exec "$vmName" "$vmZone" "$vmPublicIp" \
      "Stopping client $vmName ($count):" \
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
  ) >> "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  mv "log-$vmName.txt" "log-$pid.txt"
  pids+=("$pid")
}

fullnode_start() {
  declare class=$1
  declare vmName=$2
  declare vmZone=$3
  declare vmPublicIp=$4
  declare count=$5

  touch "log-$vmName.txt"
  (
    SECONDS=0
    commonNodeConfig="\
      rust-log=$RUST_LOG \
      default-metrics-rate=$SOLANA_DEFAULT_METRICS_RATE \
      metrics-config=$SOLANA_METRICS_CONFIG \
      setup-args=$SOLANA_SETUP_ARGS \
    "
    if [[ $class = leader ]]; then
      nodeConfig="mode=leader+drone $commonNodeConfig"
      if [[ -n $SOLANA_CUDA ]]; then
        nodeConfig="$nodeConfig enable-cuda=1"
      fi
    else
      nodeConfig="mode=validator leader-address=$publicIp $commonNodeConfig"
    fi

    vm_exec "$vmName" "$vmZone" "$vmPublicIp" "Starting $class $count:" \
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
  ) >> "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  mv "log-$vmName.txt" "log-$pid.txt"

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
  declare vmPublicIp=$3
  declare count=$4

  touch "log-$vmName.txt"
  (
    SECONDS=0
    # Try to ping the machine first.  When a machine (validator) is restarted,
    # there can be a delay between when the instance is reported as RUNNING and when
    # it's reachable over the network
    timeout 30s bash -c "set -o pipefail; until ping -c 3 $vmPublicIp | tr - _; do echo .; done"
    vm_exec "$vmName" "$vmZone" "$vmPublicIp" "Shutting down" "\
      if snap list solana; then \
        sudo snap set solana mode=; \
      fi"
    echo "Succeeded in ${SECONDS} seconds"
  ) >> "log-$vmName.txt" 2>&1 &
  declare pid=$!

  # Rename log file so it can be discovered later by $pid
  mv "log-$vmName.txt" "log-$pid.txt"

  pids+=("$pid")
}

if [[ -n $LOCAL_SNAP ]]; then
  echo "--- Transferring $LOCAL_SNAP to node(s)"

  transfer_local_snap() {
    declare vmName=$1
    declare vmZone=$2
    declare vmPublicIp=$3
    declare vmClass=$4
    declare count=$5

    echo "--- $vmName in zone $vmZone ($count)"
    SECONDS=0
    scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      "$LOCAL_SNAP" testnet-deploy@"$vmPublicIp":solana_local.snap
    echo "Succeeded in ${SECONDS} seconds"
  }
  vm_foreach transfer_local_snap
fi

echo "--- Stopping client node(s)"
pids=()
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
  snapVersion=unknown
else
  (
    set -x
    USE_SNAP=1 ci/testnet-sanity.sh $publicUrl $fullnode_count
  )
  IFS=\  read -r _ snapVersion _ < <(snap info solana | grep "^installed:")
  snapVersion=${snapVersion/0+git./}
fi

pids=("${client_stop_pids[@]}")
wait_for_pids client shutdown
vm_foreach_in_class client client_start

# Add "network started" datapoint
$metrics_write_datapoint "testnet-deploy,name=$netBasename start=1,version=\"$snapVersion\""

exit 0
