#!/usr/bin/env bash
#set -e

here=$(dirname "$0")
# shellcheck source=net/common.sh
source "$here"/common.sh

declare -gr LF="/home/solana/.solana.lock"

# Load colo resource specs
N_RES=0
RES_HOSTNAME=()
RES_IP=()
RES_IP_PRIV=()
RES_CPU_CORES=()
RES_RAM_GB=()
RES_STORAGE_TYPE=()
RES_STORAGE_CAP_GB=()
RES_ADD_STORAGE_TYPE=()
RES_ADD_STORAGE_CAP_GB=()
RES_GPUS=()
while read -r LINE; do
  IFS='|' read -r H I PI C M ST SC AST ASC G Z <<<"$LINE"
  RES_HOSTNAME+=( "$H" )
  RES_IP+=( "$I" )
  RES_IP_PRIV+=( "$PI" )
  RES_CPU_CORES+=( "$C" )
  RES_RAM_GB+=( "$M" )
  RES_STORAGE_TYPE+=( "$ST" )
  RES_STORAGE_CAP_GB+=( "$SC" )
  RES_ADD_STORAGE_TYPE+=( "$(tr ',' $'\v' <<<"$AST")" )
  RES_ADD_STORAGE_CAP_GB+=( "$(tr ',' $'\v' <<<"$ASC")" )
  RES_GPUS+=( "$G" )
  RES_ZONE+=( "$Z" )
  ((N_RES++))
done <"$here"/colo_nodes

res_index_from_ip() {
  declare IP="$1"
  for i in "${!RES_IP_PRIV[@]}"; do
    if [ "$IP" = "${RES_IP_PRIV[$i]}" ]; then
      echo "$i"
      return 0
    fi
  done
  return 1
}

instance_run() {
  declare IP=$1
  declare CMD="$2"
  declare OUT
  OUT=$(ssh -l solana -o "ConnectTimeout=3" "$IP" "$CMD" 2>&1)
  declare RC=$?
  while read -r LINE; do
    echo -e "$IP\v$RC\v$LINE"
  done <<< "$OUT"
  return $RC
}

instance_run_foreach() {
  declare CMD
  if test 1 -eq $#; then
    CMD="$1"
    declare IPS=()
    for I in $(seq 0 $((N_RES-1))); do
      IPS+=( "${RES_IP_PRIV[$I]}" )
    done
    set "${IPS[@]}" "$CMD"
  fi
  CMD="${*: -1}"
  for I in $(seq 0 $(($#-2))); do
    declare IP="$1"
    instance_run "$IP" "$CMD" &
    shift
  done

  wait
}

colo_whoami() {
  declare ME LINE SOL_USER
  while read -r LINE; do
    declare IP RC
    IFS=$'\v' read -r IP RC SOL_USER <<< "$LINE"
    if [ "$RC" -eq 0 ]; then
      if [ -z "$ME" ] || [ "$ME" = "$SOL_USER" ]; then
        ME="$SOL_USER"
      else
        echo "Found conflicting username \"$SOL_USER\" on $IP, expected \"$ME\"" 1>&2
      fi
    fi
  done < <(instance_run_foreach "[ -n \"\$SOLANA_USER\" ] && echo \"\$SOLANA_USER\"")
  echo "$ME"
}

__SOLANA_USER=""
get_solana_user() {
  if [ -z "$__SOLANA_USER" ]; then
    __SOLANA_USER=$(colo_whoami)
  fi
  echo "$__SOLANA_USER"
}

_node_status_script() {
  cat <<EOF
  exec 3>&2
  exec 2>/dev/null  # Suppress stderr as the next call to exec fails most of
                    # the time due to $LF not existing and is running from a
                    # subshell where normal redirection doesn't work
  exec 9<"$LF" && flock -s 9 && . "$LF" && exec 9>&-
  echo -e "\$SOLANA_LOCK_USER\\v\$SOLANA_LOCK_ROLE\\v\$SOLANA_LOCK_NETNAME"
  exec 2>&3 # Restore stderr
EOF
}

_node_status_result_normalize() {
  declare IP RC US RL NETNAME BY
  declare ST="DOWN"
  IFS=$'\v' read -r IP RC US RL NETNAME <<< "$1"
  if [ "$RC" -eq 0 ]; then
    if [ -n "$US" ]; then
      BY="$US"
      ST="HELD"
    else
      ST="FREE"
    fi
  fi
  echo -e $"$IP\v$ST\v$BY\v$RL\v$NETNAME"
}

node_status() {
  declare IP="$1"
  _node_status_result_normalize "$(instance_run "$IP" "$(_node_status_script)")"
}

node_status_all() {
  declare LINE
  while read -r LINE; do
    _node_status_result_normalize "$LINE"
  done < <(instance_run_foreach "$(_node_status_script)")
}

printNetworkInfo() {
  cat <<EOF
==[ Network composition ]===============================================================
  Bootstrap leader = $bootstrapLeaderMachineType (GPU=$enableGpu)
  Additional fullnodes = $additionalFullNodeCount x $fullNodeMachineType
  Client(s) = $clientNodeCount x $clientMachineType
  Replicators(s) = $replicatorNodeCount x $replicatorMachineType
  Blockstreamer = $blockstreamer
========================================================================================

EOF
}

creationInfo() {
  declare creationDate
  creationDate=$(date)
  cat <<EOF

  Instance running since: $creationDate

========================================================================================
EOF
}

requisition_node() {
  declare IP=$1
  declare ROLE=$2
  instance_run "$IP" "$(
cat <<EOF
  if [ ! -f "$LF" ]; then
    exec 9>>"$LF"
    flock -x -n 9 || exit 1
    [ -n "\$SOLANA_USER" ] && {
      echo "export SOLANA_LOCK_USER=\$SOLANA_USER"
      echo "export SOLANA_LOCK_ROLE=$ROLE"
      echo "export SOLANA_LOCK_NETNAME=$prefix"
      echo "[ -v SSH_TTY -a -f \"\${HOME}/.solana-motd\" ] && cat \"\${HOME}/.solana-motd\" 1>&2"
    } >&9 || ( rm "$LF" && false )
    9>&-
    cat > /solana-scratch/id_ecdsa <<EOK
$(cat "$sshPrivateKey")
EOK
    cat > /solana-scratch/id_ecdsa.pub <<EOK
$(cat "${sshPrivateKey}.pub")
EOK
    chmod 0600 /solana-scratch/id_ecdsa
    cat > /solana-scratch/authorized_keys <<EOAK
$("$here"/scripts/add-datacenter-solana-user-authorized_keys.sh 2> /dev/null)
$(cat "${sshPrivateKey}.pub")
EOAK
    cp /solana-scratch/id_ecdsa "\${HOME}/.ssh/id_ecdsa"
    cp /solana-scratch/id_ecdsa.pub "\${HOME}/.ssh/id_ecdsa.pub"
    cp /solana-scratch/authorized_keys "\${HOME}/.ssh/authorized_keys"
    cat > "\${HOME}/.solana-motd" <<EOM


$(printNetworkInfo)
$(creationInfo)
EOM

    # XXX: Stamp creation MUST be last!
    touch /solana-scratch/.instance-startup-complete
  else
    false
  fi
EOF
  )"
}

free_node() {
  declare IP=$1
  instance_run "$IP" "$(
cat <<EOF
  RC=false
  if [ -f "$LF" ]; then
    exec 9<>"$LF"
    flock -x -n 9 || exit 1
    . "$LF"
    if [ "\$SOLANA_LOCK_USER" = "\$SOLANA_USER" ]; then
      git clean -qxdff
      rm -f /solana-scratch/* /solana-scratch/.[^.]*
      cat > "\${HOME}/.ssh/authorized_keys" <<EOAK
$("$here"/scripts/add-datacenter-solana-user-authorized_keys.sh 2> /dev/null)
EOAK
      RC=true
    fi
    9>&-
  fi
  \$RC
EOF
  )"
}

cloud_FindInstances() {
  declare LINE PRIV_IP STATUS LOCK_USER ROLE NETNAME I IP HOST_NAME ZONE
  declare QRY_NETNAME="$1"
  declare QRY_ROLE="$2"
  instances=()
  while read -r LINE; do
    IFS=$'\v' read -r PRIV_IP STATUS LOCK_USER ROLE NETNAME <<< "$LINE"
    I=$(res_index_from_ip "$PRIV_IP")
    IP="${RES_IP[$I]}"
    HOST_NAME="${RES_HOSTNAME[$I]}"
    ZONE="${RES_ZONE[$I]}"
    if [[ -z "$QRY_NETNAME" || "$NETNAME" = "$QRY_NETNAME" ]] && [[ -z "$QRY_ROLE" || "$ROLE" = "$QRY_ROLE" ]]; then
      IP=$PRIV_IP  # Colo public IPs are firewalled to only allow UDP(8000-10000).  Reuse private IP as public and require VPN
      printf "%-30s | publicIp=%-16s privateIp=%s status=%s zone=%s net=%s role=%s\n" "$HOST_NAME" "$IP" "$PRIV_IP" "$STATUS" "$ZONE" "$NETNAME" "$ROLE"
      instances+=( "$(echo -e "$HOST_NAME\v$IP\v$PRIV_IP\v$STATUS\v$ZONE\v$LOCK_USER\v$ROLE\v$NETNAME")" )
    fi
  done < <(node_status_all | sort -t $'\v' -k1)
}

cpuBootstrapLeaderMachineType=0
gpuBootstrapLeaderMachineType=1
bootstrapLeaderMachineType=$cpuBootstrapLeaderMachineType
fullNodeMachineType=$cpuBootstrapLeaderMachineType
clientMachineType=0
blockstreamerMachineType=0
replicatorMachineType=0

# FIXME: Use `get_solana_user` instead of $USER from the calling shell
prefix=testnet-dev-${USER//[^A-Za-z0-9]/}
additionalFullNodeCount=2
clientNodeCount=1
replicatorNodeCount=0
blockstreamer=false
externalNodes=false

publicNetwork=false
enableGpu=false

usage() {
  declare exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|config|delete] [common options] [command-specific options]

Manage testnet instances

 create - create a new testnet (implies 'config')
 config - configure the testnet and write a config file describing it
 delete - delete the testnet
 info   - display information about the currently configured testnet
 status - display information about current resource allocations
 whoami - display your assigned colo network username

 common options:
   -p [prefix]      - Optional common prefix for instance names to avoid
                      collisions (default: $prefix)
   -x               - append to the existing configuration instead of creating a
                      new configuration
   -f               - Discard validator nodes that didn't bootup successfully

 create-specific options:
   -n [number]      - Number of additional fullnodes (default: $additionalFullNodeCount)
   -c [number]      - Number of client nodes (default: $clientNodeCount)
   -r [number]      - Number of replicator nodes (default: $replicatorNodeCount)
   -u               - Include a Blockstreamer (default: $blockstreamer)
   -P               - Use public network IP addresses (default: $publicNetwork)
   -g               - Enable GPU (default: $enableGpu)
   -G               - Enable GPU, and set count/type of GPUs to use
                      (e.g $gpuBootstrapLeaderMachineType)
   -a [address]     - Address to be be assigned to the Blockstreamer if present,
                      otherwise the bootstrap fullnode.
                      * For GCE, [address] is the "name" of the desired External
                        IP Address.
                      * For EC2, [address] is the "allocation ID" of the desired
                        Elastic IP.
 config-specific options:
   -P               - Use public network IP addresses (default: $publicNetwork)

 delete-specific options:
   none

 info-specific options:
   none

EOF
  exit $exitcode
}

command=$1
[[ -n $command ]] || usage
shift
[[ $command = create || $command = config || $command = info || $command = delete || $command = status || $command = whoami ]] ||
  usage "Invalid command: $command"

shortArgs=()
while [[ -n $1 ]]; do
  if [[ ${1:0:2} = -- ]]; then
    if [[ $1 == --machine-type* ]]; then # Bypass quoted long args for GPUs
      shortArgs+=("$1")
      shift
    else
      usage "Unknown long option: $1"
    fi
  else
    shortArgs+=("$1")
    shift
  fi
done

while getopts "h?Pp:n:c:r:gG:uxf" opt "${shortArgs[@]}"; do
  case $opt in
  h | \?)
    usage
    ;;
  p)
    [[ ${OPTARG//[^A-Za-z0-9-]/} == "$OPTARG" ]] || usage "Invalid prefix: \"$OPTARG\", alphanumeric only"
    prefix=$OPTARG
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
  r)
    replicatorNodeCount=$OPTARG
    ;;
  g)
    enableGpu=true
    bootstrapLeaderMachineType=$gpuBootstrapLeaderMachineType
    fullNodeMachineType=$bootstrapLeaderMachineType
    blockstreamerMachineType=$bootstrapLeaderMachineType
    ;;
  G)
    enableGpu=true
    bootstrapLeaderMachineType="$OPTARG"
    fullNodeMachineType=$bootstrapLeaderMachineType
    blockstreamerMachineType=$bootstrapLeaderMachineType
    ;;
  u)
    blockstreamer=true
    ;;
  x)
    # TODO: There's no SSH on the Colo public IPs, which net.sh requires
    # to rsync deployment binaries.  Need to add support throughout infrastructure
    # to specify the SSH IP independently
    echo "External nodes not yet supported!"
    exit 1
    # externalNodes=true
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done

[[ -z $1 ]] || usage "Unexpected argument: $1"

sshPrivateKey="$netConfigDir/id_$prefix"

# cloud_ForEachInstance [cmd] [extra args to cmd]
#
# Execute a command for each element in the `instances` array
#
#   cmd   - The command to execute on each instance
#           The command will receive arguments followed by any
#           additional arguments supplied to cloud_ForEachInstance:
#               name     - name of the instance
#               publicIp - The public IP address of this instance
#               privateIp - The private IP address of this instance
#               count    - Monotonically increasing count for each
#                          invocation of cmd, starting at 1
#               ...      - Extra args to cmd..
#
#
cloud_ForEachInstance() {
  declare cmd="$1"
  shift
  [[ -n $cmd ]] || { echo cloud_ForEachInstance: cmd not specified; exit 1; }

  declare count=1
  for info in "${instances[@]}"; do
    declare name publicIp privateIp
    IFS=$'\v' read -r name publicIp privateIp _ <<< "$info"

    eval "$cmd" "$name" "$publicIp" "$privateIp" "$count" "$@"
    count=$((count + 1))
  done
}

prepareInstancesAndWriteConfigFile() {
  $metricsWriteDatapoint "testnet-deploy net-config-begin=1"

  if $externalNodes; then
    echo "Appending to existing config file"
    echo "externalNodeSshKey=$sshPrivateKey" >> "$configFile"
  else
    rm -f "$geoipConfigFile"

    cat >> "$configFile" <<EOF
# autogenerated at $(date)
netBasename=$prefix
publicNetwork=$publicNetwork
sshPrivateKey=$sshPrivateKey
EOF
  fi
  touch "$geoipConfigFile"

  buildSshOptions

  fetchPrivateKey() {
    declare nodeName
    declare nodeIp
    IFS=$'\v' read -r nodeName nodeIp _ <<< "${instances[0]}"

    # Make sure the machine is alive or pingable
    timeout_sec=90
    cloud_WaitForInstanceReady "$nodeName" "$nodeIp" "$timeout_sec"

    if [[ ! -r $sshPrivateKey ]]; then
      echo "Fetching $sshPrivateKey from $nodeName"

      # Try to scp in a couple times, sshd may not yet be up even though the
      # machine can be pinged...
      (
        set -o pipefail
        for i in $(seq 1 60); do
          set -x
          cloud_FetchFile "$nodeName" "$nodeIp" /solana-scratch/id_ecdsa "$sshPrivateKey" &&
            cloud_FetchFile "$nodeName" "$nodeIp" /solana-scratch/id_ecdsa.pub "$sshPrivateKey.pub" &&
              break
          set +x

          sleep 1
          echo "Retry $i..."
        done
      )

      chmod 400 "$sshPrivateKey"
      ls -l "$sshPrivateKey"
    fi
  }

  recordInstanceIp() {
    declare name="$1"
    declare publicIp="$2"
    declare privateIp="$3"

    declare arrayName="$6"

    {
      echo "$arrayName+=($publicIp)  # $name"
      echo "${arrayName}Private+=($privateIp)  # $name"
    } >> "$configFile"
  }

  if $externalNodes; then
    echo "Bootstrap leader is already configured"
  else
    echo "Looking for bootstrap leader instance..."
    cloud_FindInstances "$prefix" "bootstrap-leader"
    [[ ${#instances[@]} -eq 1 ]] || {
      echo "Unable to find bootstrap leader"
      exit 1
    }

    echo "fullnodeIpList=()" >> "$configFile"
    echo "fullnodeIpListPrivate=()" >> "$configFile"
    cloud_ForEachInstance recordInstanceIp true fullnodeIpList
  fi

  if ! $externalNodes; then
    echo "clientIpList=()" >> "$configFile"
    echo "clientIpListPrivate=()" >> "$configFile"
  fi
  echo "Looking for client bencher instances..."
  cloud_FindInstances "$prefix" "client"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp true clientIpList
  }

  if ! $externalNodes; then
    echo "blockstreamerIpList=()" >> "$configFile"
    echo "blockstreamerIpListPrivate=()" >> "$configFile"
  fi
  echo "Looking for blockstreamer instances..."
  cloud_FindInstances "$prefix" "blockstreamer"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp true blockstreamerIpList
  }

  if [[ $additionalFullNodeCount -gt 0 ]]; then
    cloud_FindInstances "$prefix" "fullnode"
    if [[ ${#instances[@]} -gt 0 ]]; then
      cloud_ForEachInstance recordInstanceIp true fullnodeIpList
    fi
  fi

  if ! $externalNodes; then
    echo "replicatorIpList=()" >> "$configFile"
    echo "replicatorIpListPrivate=()" >> "$configFile"
  fi
  echo "Looking for replicator instances..."
  cloud_FindInstances "$prefix" "replicator"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp true replicatorIpList
  }

  echo "Wrote $configFile"
  $metricsWriteDatapoint "testnet-deploy net-config-complete=1"
}

delete() {
  declare SOLANA_USER
  SOLANA_USER=$(get_solana_user)

  # Filter for all nodes
  declare filter="$prefix"

  echo "Searching for instances: $filter"
  cloud_FindInstances "$filter"

  $metricsWriteDatapoint "testnet-deploy net-delete-begin=1"

  declare OUT
  OUT=$(
    for instance in "${instances[@]}"; do
      # shellcheck disable=SC2034
      IFS=$'\v' read -r _HOST_NAME _IP PRIV_IP _STATUS _ZONE HOLD_USER ROLE _NETNAME <<< "$instance"
      if [ "$HOLD_USER" = "$SOLANA_USER" ]; then
        ( free_node "$PRIV_IP" | sed -e "s/$/$ROLE/" ) &
      fi
    done
    wait
  )

  if [ -n "$OUT" ]; then
    declare LINE IP RC OTHER
    while read -r LINE; do
      IFS=$'\v' read -r IP RC OTHER <<< "$LINE"
      if [ "$RC" -eq 0 ]; then
        declare ROLE="$OTHER"
        echo "Deleted $IP($ROLE)"
      else
        declare REASON="$OTHER"
        echo "Failed to delete $IP. $REASON"
      fi
    done < <(sort -t $'\v' -k1 <<< "$OUT")
  else
    echo "There are no resources owned by \"$SOLANA_USER\" to delete!"
  fi

  echo "Searching for instances:"
  cloud_FindInstances

  if $externalNodes; then
    echo "Let's not delete the current configuration file"
  else
    rm -f "$configFile"
  fi

  $metricsWriteDatapoint "testnet-deploy net-delete-complete=1"
}

case $command in
delete)
  delete
  ;;

create)
  [[ -n $additionalFullNodeCount ]] || usage "Need number of nodes"

  # Build resource request list form CLI options
  RES_REQUEST=()
  REQ_TEXT="$(
  {
    if $blockstreamer; then
      echo -e "$blockstreamerMachineType\vblockstreamer"
    fi
    echo -e "$bootstrapLeaderMachineType\vbootstrap-leader"
    for _ in $(seq "$additionalFullNodeCount"); do
      echo -e "$fullNodeMachineType\vfullnode"
    done
    for _ in $(seq "$clientNodeCount"); do
      echo -e "$clientMachineType\vclient"
    done
    for _ in $(seq "$replicatorNodeCount"); do
      echo -e "$replicatorMachineType\vreplicator"
    done
  } | sort -rnt $'\v' -k1,1)"
  while read -r LINE; do
    RES_REQUEST+=("$LINE")
  done <<<"$REQ_TEXT"

  delete  # XXX: delete populates $(instances) :/

  # Query available resources
  RES_AVAIL=()
  AVL_TEXT="$(
    for i in "${instances[@]}"; do
      IFS=$'\v' read -r _ _ IP ST _ <<< "$i"
      if [ "FREE" = "$ST" ]; then
        IDX=$(res_index_from_ip "$IP")
        NGPU=${RES_GPUS[$IDX]}
        echo -e "$NGPU\v$IDX\v$IP"
      fi
    done | sort -nt $'\v' -k1,1
  )"
  while read -r LINE; do
    RES_AVAIL+=( "$LINE" )
  done <<<"$AVL_TEXT"

  # Build allocation candidate list from Union(requested, available)
  TO_ALLOC=()
  USED=()
  for r in "${RES_REQUEST[@]}"; do
    SUCCESS=false
    IFS=$'\v' read -r REQ_GPUS REQ_ROLE <<< "$r"
    for a in "${RES_AVAIL[@]}"; do
      IFS=$'\v' read -r AVAIL_GPUS AVAIL_IDX AVAIL_IP <<< "$a"
      SKIP=false
      for u in "${USED[@]}"; do
        if [ "$u" -eq "$AVAIL_IDX" ]; then
          SKIP=true
          break
        fi
      done
      if ! $SKIP; then
        if [ "$AVAIL_GPUS" -ge "$REQ_GPUS" ]; then
          TO_ALLOC+=( "$(echo -e "$REQ_ROLE\v$AVAIL_IDX\v$AVAIL_IP")" )
          USED+=( "$AVAIL_IDX" )
          SUCCESS=true
          break
        fi
      fi
    done
    if ! $SUCCESS; then
      echo "No available resources for a ${REQ_ROLE} with ${REQ_GPUS} GPUs"
      break
    fi
  done

  if [ ${#RES_REQUEST[@]} -gt ${#TO_ALLOC[@]} ]; then
    echo "There are insufficient free resources to fulfill the requested configuration"
    exit 1
  fi

  $metricsWriteDatapoint "testnet-deploy net-create-begin=1"

  rm -rf "$sshPrivateKey"{,.pub}

  # Note: using rsa because |aws ec2 import-key-pair| seems to fail for ecdsa
  ssh-keygen -t rsa -N '' -f "$sshPrivateKey"

  # Attempt to requistion resources
  if [ ${#TO_ALLOC[@]} -gt 0 ]; then
    RES=$(
      for r in "${TO_ALLOC[@]}"; do
        IFS=$'\v' read -r ROLE IDX IP <<< "$r"
        ( requisition_node "$IP" "$ROLE" | sed "s/$/$ROLE\v$IDX/" ) &
      done; wait
    )
    USED=()
    ROLLBACK=false
    while read -r LINE; do
      IFS=$'\v' read -r IP RC ROLE IDX <<< "$LINE"
      if [ "$RC" -eq 0 ]; then
        echo "Requisitioned $IP as $ROLE"
        USED+=( "$IP" )
      else
        echo "Failed to requisition $IP as $ROLE"
        ROLLBACK=true
      fi
    done < <(sort -t $'\v' -k1 <<< "$RES")

    if $ROLLBACK; then
      for u in "${USED[@]}"; do
        free_node "$u" &
      done
      wait
      exit 1
    fi
  fi

  $metricsWriteDatapoint "testnet-deploy net-create-complete=1"

  prepareInstancesAndWriteConfigFile
  ;;

config)
  prepareInstancesAndWriteConfigFile
  ;;
info)
  loadConfigFile
  printNode() {
    declare nodeType=$1
    declare ip=$2
    declare ipPrivate=$3
    printf "  %-16s | %-15s | %-15s\n" "$nodeType" "$ip" "$ipPrivate"
  }

  printNode "Node Type" "Public IP" "Private IP"
  echo "-------------------+-----------------+-----------------"
  nodeType=bootstrap-leader
  for i in $(seq 0 $(( ${#fullnodeIpList[@]} - 1)) ); do
    ipAddress=${fullnodeIpList[$i]}
    ipAddressPrivate=${fullnodeIpListPrivate[$i]}
    printNode $nodeType "$ipAddress" "$ipAddressPrivate"
    nodeType=fullnode
  done

  for i in $(seq 0 $(( ${#clientIpList[@]} - 1)) ); do
    ipAddress=${clientIpList[$i]}
    ipAddressPrivate=${clientIpListPrivate[$i]}
    printNode client "$ipAddress" "$ipAddressPrivate"
  done

  for i in $(seq 0 $(( ${#blockstreamerIpList[@]} - 1)) ); do
    ipAddress=${blockstreamerIpList[$i]}
    ipAddressPrivate=${blockstreamerIpListPrivate[$i]}
    printNode blockstreamer "$ipAddress" "$ipAddressPrivate"
  done

  for i in $(seq 0 $(( ${#replicatorIpList[@]} - 1)) ); do
    ipAddress=${replicatorIpList[$i]}
    ipAddressPrivate=${replicatorIpListPrivate[$i]}
    printNode replicator "$ipAddress" "$ipAddressPrivate"
  done
  ;;
status)
  cloud_FindInstances
  ;;
whoami)
  echo "  You are: \"$(get_solana_user)\""
  ;;
*)
  usage "Unknown command: $command"
esac
