#!/usr/bin/env bash

# |source| this file
#
# Utilities for working with Colo instances
#

declare -r SOLANA_LOCK_FILE="/home/solana/.solana.lock"

declare __COLO_TODO_PARALLELIZE=false

# Load colo resource specs
export N_RES=0
export RES_HOSTNAME=()
export RES_IP=()
export RES_IP_PRIV=()
export RES_CPU_CORES=()
export RES_RAM_GB=()
export RES_STORAGE_TYPE=()
export RES_STORAGE_CAP_GB=()
export RES_ADD_STORAGE_TYPE=()
export RES_ADD_STORAGE_CAP_GB=()
export RES_GPUS=()

export __COLO_RESOURCES_LOADED=false
load_resources() {
  if ! ${__COLO_RESOURCES_LOADED}; then
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
      N_RES=$((N_RES+1))
    done < <(sort -nt'|' -k10,10 "$here"/colo_nodes)
    __COLO_RESOURCES_LOADED=true
  fi
}

declare __COLO_RES_AVAILABILITY_CACHED=false
declare -ax __COLO_RES_AVAILABILITY
load_availability() {
  declare USE_CACHE=${1:-${__COLO_RES_AVAILABILITY_CACHED}}
  declare LINE PRIV_IP STATUS LOCK_USER ROLE NETNAME I IP HOST_NAME ZONE INSTNAME
  if ! $USE_CACHE; then
    __COLO_RES_AVAILABILITY=()
    __COLO_RES_REQUISITIONED=()
    while read -r LINE; do
      IFS=$'\v' read -r PRIV_IP STATUS LOCK_USER INSTNAME ROLE NETNAME <<< "$LINE"
      I=$(res_index_from_ip "$PRIV_IP")
      IP="${RES_IP[$I]}"
      HOST_NAME="${RES_HOSTNAME[$I]}"
      ZONE="${RES_ZONE[$I]}"
      IP=$PRIV_IP  # Colo public IPs are firewalled to only allow UDP(8000-10000).  Reuse private IP as public and require VPN
      __COLO_RES_AVAILABILITY+=( "$(echo -e "$HOST_NAME\v$IP\v$PRIV_IP\v$STATUS\v$ZONE\v$LOCK_USER\v$ROLE\v$NETNAME\v$INSTNAME")" )
    done < <(node_status_all | sort -t $'\v' -k1)
    __COLO_RES_AVAILABILITY_CACHED=true
  fi
}

print_availability() {
  declare HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME
  if ! $__COLO_TODO_PARALLELIZE; then
    load_resources
    load_availability false
  fi
  for AVAIL in "${__COLO_RES_AVAILABILITY[@]}"; do
    IFS=$'\v' read -r HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME <<<"$AVAIL"
    printf "%-30s | publicIp=%-16s privateIp=%s status=%s zone=%s net=%s role=%s inst=%s\n" "$HOST_NAME" "$IP" "$PRIV_IP" "$STATUS" "$ZONE" "$NETNAME" "$ROLE" "$INSTNAME"
  done
}

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
  OUT=$(ssh -l solana -o "ConnectTimeout=10" "$IP" "$CMD" 2>&1)
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
                    # the time due to $SOLANA_LOCK_FILE not existing and is running from a
                    # subshell where normal redirection doesn't work
  exec 9<"$SOLANA_LOCK_FILE" && flock -s 9 && . "$SOLANA_LOCK_FILE" && exec 9>&-
  echo -e "\$SOLANA_LOCK_USER\\v\$SOLANA_LOCK_INSTANCENAME\\v\$SOLANA_LOCK_ROLE\\v\$SOLANA_LOCK_NETNAME"
  exec 2>&3 # Restore stderr
EOF
}

_node_status_result_normalize() {
  declare IP RC US RL NETNAME BY INSTNAME
  declare ST="DOWN"
  IFS=$'\v' read -r IP RC US INSTNAME RL NETNAME <<< "$1"
  if [ "$RC" -eq 0 ]; then
    if [ -n "$US" ]; then
      BY="$US"
      ST="HELD"
    else
      ST="FREE"
    fi
  fi
  echo -e $"$IP\v$ST\v$BY\v$INSTNAME\v$RL\v$NETNAME"
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

# TODO: As part of __COLO_TOOD_PARALLELIZE this list will need to be maintained
# in a lockfile to work around `cloud_CreateInstance` being called in the
# background for fullnodes
export __COLO_RES_REQUISITIONED=()
requisition_node() {
  declare IP=$1
  declare ROLE=$2
  declare INSTANCE_NAME=$3

  declare INDEX=$(res_index_from_ip "$IP")
  declare RC=false

  instance_run "$IP" "$(
cat <<EOF
  if [ ! -f "$SOLANA_LOCK_FILE" ]; then
    exec 9>>"$SOLANA_LOCK_FILE"
    flock -x -n 9 || exit 1
    [ -n "\$SOLANA_USER" ] && {
      echo "export SOLANA_LOCK_USER=\$SOLANA_USER"
      echo "export SOLANA_LOCK_INSTANCENAME=$INSTANCE_NAME"
      echo "export SOLANA_LOCK_ROLE=$ROLE"
      echo "export SOLANA_LOCK_NETNAME=$prefix"
      echo "[ -v SSH_TTY -a -f \"\${HOME}/.solana-motd\" ] && cat \"\${HOME}/.solana-motd\" 1>&2"
    } >&9 || ( rm "$SOLANA_LOCK_FILE" && false )
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
  if [[ 0 -eq $? ]]; then
    __COLO_RES_REQUISITIONED+=("$INDEX")
    RC=true
  fi
  $RC
}

node_is_requisitioned() {
  declare INDEX="$1"
  declare REQ
  declare RC=false
  for REQ in "${__COLO_RES_REQUISITIONED[@]}"; do
    if [[ $REQ -eq $INDEX ]]; then
      RC=true
      break
    fi
  done
  $RC
}

machine_types_compatible() {
  declare MAYBE_MACH="$1"
  declare WANT_MACH="$2"
  declare COMPATIBLE=false
  # XXX: Colo machine types are just GPU count ATM...
  if [[ "$MAYBE_MACH" -ge "$WANT_MACH" ]]; then
    COMPATIBLE=true
  fi
  $COMPATIBLE
}

free_node() {
  declare IP=$1
  instance_run "$IP" "$(
cat <<EOF
  RC=false
  if [ -f "$SOLANA_LOCK_FILE" ]; then
    exec 9<>"$SOLANA_LOCK_FILE"
    flock -x -n 9 || exit 1
    . "$SOLANA_LOCK_FILE"
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


# Default zone
cloud_DefaultZone() {
  echo "Denver"
}

#
# __cloud_FindInstances
#
# Find instances matching the specified pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:zone:public IP:private IP"
#
# filter   - The instances to filter on
#
# examples:
#   $ __cloud_FindInstances "name=exact-machine-name"
#   $ __cloud_FindInstances "name~^all-machines-with-a-common-machine-prefix"
#
__cloud_FindInstances() {
  declare HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME INSTANCES_TEXT
  declare filter=$1
  instances=()

  if ! $__COLO_TODO_PARALLELIZE; then
    load_resources
    load_availability false
  fi
  INSTANCES_TEXT="$(
    for AVAIL in "${__COLO_RES_AVAILABILITY[@]}"; do
      IFS=$'\v' read -r HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME <<<"$AVAIL"
      if [[ $INSTNAME =~ $filter ]]; then
        IP=$PRIV_IP  # Colo public IPs are firewalled to only allow UDP(8000-10000).  Reuse private IP as public and require VPN
        printf "%-30s | publicIp=%-16s privateIp=%s status=%s zone=%s net=%s role=%s inst=%s\n" "$HOST_NAME" "$IP" "$PRIV_IP" "$STATUS" "$ZONE" "$NETNAME" "$ROLE" "$INSTNAME" 1>&2
        echo -e "${INSTNAME}:${IP}:${PRIV_IP}:$ZONE"
      fi
    done | sort -t $'\v' -k1
  )"
  if [[ -n "$INSTANCES_TEXT" ]]; then
    while read -r LINE; do
      instances+=( "$LINE" )
    done <<<"$INSTANCES_TEXT"
  fi
}

#
# cloud_FindInstances [namePrefix]
#
# Find instances with names matching the specified prefix
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP"
#
# namePrefix - The instance name prefix to look for
#
# examples:
#   $ cloud_FindInstances all-machines-with-a-common-machine-prefix
#
cloud_FindInstances() {
  declare filter="^${1}.*"
  __cloud_FindInstances "$filter"
}

#
# cloud_FindInstance [name]
#
# Find an instance with a name matching the exact pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP"
#
# name - The instance name to look for
#
# examples:
#   $ cloud_FindInstance exact-machine-name
#
cloud_FindInstance() {
  declare name="^${1}$"
  __cloud_FindInstances "$name"
}

#
# cloud_Initialize [networkName]
#
# Perform one-time initialization that may be required for the given testnet.
#
# networkName   - unique name of this testnet
#
# This function will be called before |cloud_CreateInstances|
cloud_Initialize() {
  networkName=$1
  zone=$2
  load_resources
  if $__COLO_TODO_PARALLELIZE; then
    load_availability
  fi
}

#
# cloud_CreateInstances [networkName] [namePrefix] [numNodes] [imageName]
#                       [machineType] [bootDiskSize] [enableGpu]
#                       [startupScript] [address]
#
# Creates one more identical instances.
#
# networkName   - unique name of this testnet
# namePrefix    - unique string to prefix all the instance names with
# numNodes      - number of instances to create
# imageName     - Disk image for the instances
# machineType   - GCE machine type.  Note that this may also include an
#                 `--accelerator=` or other |gcloud compute instances create|
#                 options
# bootDiskSize  - Optional size of the boot disk in GB
# enableGpu     - Optionally enable GPU, use the value "true" to enable
#                 eg, request 4 K80 GPUs with "count=4,type=nvidia-tesla-k80"
# startupScript - Optional startup script to execute when the instance boots
# address       - Optional name of the GCE static IP address to attach to the
#                 instance.  Requires that |numNodes| = 1 and that addressName
#                 has been provisioned in the GCE region that is hosting `$zone`
#
# Tip: use cloud_FindInstances to locate the instances once this function
#      returns
cloud_CreateInstances() {
  declare networkName="$1"
  declare namePrefix="$2"
  declare numNodes="$3"
  declare enableGpu="$4"
  declare machineType="$5"
  declare zone="$6"
  declare optionalBootDiskSize="$7"
  declare optionalStartupScript="$8"
  declare optionalAddress="$9"
  declare optionalBootDiskType="${10}"
  declare optionalAdditionalDiskSize="${11}"

  declare -a nodes
  if [[ $numNodes = 1 ]]; then
    nodes=("$namePrefix")
  else
    for node in $(seq -f "${namePrefix}%0${#numNodes}g" 1 "$numNodes"); do
      nodes+=("$node")
    done
  fi

  if $__COLO_TODO_PARALLELIZE; then
    declare HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME INDEX MACH RES LINE
    declare -a AVAILABLE
    declare AVAILABLE_TEXT
    AVAILABLE_TEXT="$(
      for RES in "${__COLO_RES_AVAILABILITY[@]}"; do
        IFS=$'\v' read -r HOST_NAME IP PRIV_IP STATUS ZONE LOCK_USER ROLE NETNAME INSTNAME <<<"$RES"
        if [[ "FREE" = "$STATUS" ]]; then
          INDEX=$(res_index_from_ip "$IP")
          RES_MACH="${RES_GPUS[$INDEX]}"
          if machine_types_compatible "$RES_MACH" "$machineType"; then
            if ! node_is_requisitioned "$INDEX" "${__COLO_RES_REQUISITIONED[*]}"; then
              echo -e "$RES_MACH\v$IP"
            fi
          fi
        fi
      done | sort -nt $'\v' -k1,1
    )"

    if [[ -n "$AVAILABLE_TEXT" ]]; then
      while read -r LINE; do
        AVAILABLE+=("$LINE")
      done <<<"$AVAILABLE_TEXT"
    fi

    if [[ ${#AVAILABLE[@]} -lt $numNodes ]]; then
      echo "Insufficient resources available to allocate $numNodes $namePrefix" 1>&2
      exit 1
    fi

    declare _RES_MACH node
    declare AI=0
    for node in "${nodes[@]}"; do
      IFS=$'\v' read -r _RES_MACH IP <<<"${AVAILABLE[$AI]}"
      requisition_node "$IP" role "$node" >/dev/null
      AI=$((AI+1))
    done
  else
    declare RES_MACH node
    declare RI=0
    declare NI=0
    while [[ $NI -lt $numNodes && $RI -lt $N_RES ]]; do
      node="${nodes[$NI]}"
      RES_MACH="${RES_GPUS[$RI]}"
      IP="${RES_IP_PRIV[$RI]}"
      if machine_types_compatible "$RES_MACH" "$machineType"; then
        if requisition_node "$IP" role "$node" >/dev/null; then
          NI=$((NI+1))
        fi
      fi
      RI=$((RI+1))
    done
  fi
}

#
# cloud_DeleteInstances
#
# Deletes all the instances listed in the `instances` array
#
cloud_DeleteInstances() {
  declare _INSTNAME IP _PRIV_IP _ZONE
  for instance in "${instances[@]}"; do
    IFS=':' read -r _INSTNAME IP _PRIV_IP _ZONE <<< "$instance"
    free_node "$IP" >/dev/null
  done
}

#
# cloud_WaitForInstanceReady [instanceName] [instanceIp] [instanceZone] [timeout]
#
# Return once the newly created VM instance is responding.  This function is cloud-provider specific.
#
cloud_WaitForInstanceReady() {
  declare instanceName="$1"
  declare instanceIp="$2"
  declare timeout="$4"

  timeout "${timeout}"s bash -c "set -o pipefail; until ping -c 3 $instanceIp | tr - _; do echo .; done"
}

#
# cloud_FetchFile [instanceName] [publicIp] [remoteFile] [localFile]
#
# Fetch a file from the given instance.  This function uses a cloud-specific
# mechanism to fetch the file
#
cloud_FetchFile() {
  declare instanceName="$1"
  declare publicIp="$2"
  declare remoteFile="$3"
  declare localFile="$4"
  declare zone="$5"
  scp \
    -o "StrictHostKeyChecking=no" \
    -o "UserKnownHostsFile=/dev/null" \
    -o "User=solana" \
    -o "LogLevel=ERROR" \
    -F /dev/null \
    "solana@$publicIp:$remoteFile" "$localFile"
}

cloud_StatusAll() {
  print_availability
}
