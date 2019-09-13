#!/usr/bin/env bash

# |source| this file
#
# Utilities for working with Colo instances
#

declare -r SOLANA_LOCK_FILE="/home/solana/.solana.lock"

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

load_resources



load_resouces() {
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
                    # the time due to $SOLANA_LOCK_FILE not existing and is running from a
                    # subshell where normal redirection doesn't work
  exec 9<"$SOLANA_LOCK_FILE" && flock -s 9 && . "$SOLANA_LOCK_FILE" && exec 9>&-
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

requisition_node() {
  declare IP=$1
  declare ROLE=$2
  instance_run "$IP" "$(
cat <<EOF
  if [ ! -f "$SOLANA_LOCK_FILE" ]; then
    exec 9>>"$SOLANA_LOCK_FILE"
    flock -x -n 9 || exit 1
    [ -n "\$SOLANA_USER" ] && {
      echo "export SOLANA_LOCK_USER=\$SOLANA_USER"
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
  true
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
  true
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
  true
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
  true
}

#
# cloud_DeleteInstances
#
# Deletes all the instances listed in the `instances` array
#
cloud_DeleteInstances() {
  true
}

#
# cloud_WaitForInstanceReady [instanceName] [instanceIp] [instanceZone] [timeout]
#
# Return once the newly created VM instance is responding.  This function is cloud-provider specific.
#
cloud_WaitForInstanceReady() {
  true
}

#
# cloud_FetchFile [instanceName] [publicIp] [remoteFile] [localFile]
#
# Fetch a file from the given instance.  This function uses a cloud-specific
# mechanism to fetch the file
#
cloud_FetchFile() {
  true
}

#
# cloud_CreateAndAttachPersistentDisk [instanceName] [diskSize] [diskType]
#
# Create a persistent disk and attach it to a pre-existing VM instance.
# Set disk to auto-delete upon instance deletion
#
cloud_CreateAndAttachPersistentDisk() {
  true
}
