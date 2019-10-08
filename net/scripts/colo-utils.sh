#!/usr/bin/env bash

declare -r SOLANA_LOCK_FILE="/home/solana/.solana.lock"

__colo_here="$(dirname "${BASH_SOURCE[0]}")"
# Load colo resource specs
export COLO_RES_N=0
export COLO_RES_HOSTNAME=()
export COLO_RES_IP=()
export COLO_RES_IP_PRIV=()
export COLO_RES_CPU_CORES=()
export COLO_RES_RAM_GB=()
export COLO_RES_STORAGE_TYPE=()
export COLO_RES_STORAGE_CAP_GB=()
export COLO_RES_ADD_STORAGE_TYPE=()
export COLO_RES_ADD_STORAGE_CAP_GB=()
export COLO_RES_MACHINE=()

export COLO_RESOURCES_LOADED=false
colo_load_resources() {
  if ! ${COLO_RESOURCES_LOADED}; then
    while read -r LINE; do
      IFS='|' read -r H I PI C M ST SC AST ASC G Z <<<"$LINE"
      COLO_RES_HOSTNAME+=( "$H" )
      COLO_RES_IP+=( "$I" )
      COLO_RES_IP_PRIV+=( "$PI" )
      COLO_RES_CPU_CORES+=( "$C" )
      COLO_RES_RAM_GB+=( "$M" )
      COLO_RES_STORAGE_TYPE+=( "$ST" )
      COLO_RES_STORAGE_CAP_GB+=( "$SC" )
      COLO_RES_ADD_STORAGE_TYPE+=( "$(tr ',' $'\v' <<<"$AST")" )
      COLO_RES_ADD_STORAGE_CAP_GB+=( "$(tr ',' $'\v' <<<"$ASC")" )
      COLO_RES_MACHINE+=( "$G" )
      COLO_RES_ZONE+=( "$Z" )
      COLO_RES_N=$((COLO_RES_N+1))
    done < <(sort -nt'|' -k10,10 "$__colo_here"/colo_nodes)
    COLO_RESOURCES_LOADED=true
  fi
}

declare COLO_RES_AVAILABILITY_CACHED=false
declare -ax COLO_RES_AVAILABILITY
colo_load_availability() {
  declare USE_CACHE=${1:-${COLO_RES_AVAILABILITY_CACHED}}
  declare LINE PRIV_IP STATUS LOCK_USER I IP HOST_NAME ZONE INSTNAME
  if ! $USE_CACHE; then
    COLO_RES_AVAILABILITY=()
    COLO_RES_REQUISITIONED=()
    while read -r LINE; do
      IFS=$'\v' read -r IP STATUS LOCK_USER INSTNAME <<< "$LINE"
      I=$(colo_res_index_from_ip "$IP")
      PRIV_IP="${COLO_RES_IP_PRIV[$I]}"
      HOST_NAME="${COLO_RES_HOSTNAME[$I]}"
      ZONE="${COLO_RES_ZONE[$I]}"
      COLO_RES_AVAILABILITY+=( "$(echo -e "$HOST_NAME\v$IP\v$PRIV_IP\v$STATUS\v$ZONE\v$LOCK_USER\v$INSTNAME")" )
    done < <(colo_node_status_all | sort -t $'\v' -k1)
    COLO_RES_AVAILABILITY_CACHED=true
  fi
}

colo_res_index_from_ip() {
  declare IP="$1"
  for i in "${!COLO_RES_IP_PRIV[@]}"; do
    if [[ "$IP" = "${COLO_RES_IP[$i]}" || "$IP" = "${COLO_RES_IP_PRIV[$i]}" ]]; then
      echo "$i"
      return 0
    fi
  done
  return 1
}

colo_instance_run() {
  declare IP=$1
  declare CMD="$2"
  declare OUT
  set +e
  OUT=$(ssh -l solana -o "StrictHostKeyChecking=no" -o "ConnectTimeout=3" -n "$IP" "$CMD" 2>&1)
  declare RC=$?
  set -e
  while read -r LINE; do
    echo -e "$IP\v$RC\v$LINE"
  done <<< "$OUT"
  return $RC
}

colo_instance_run_foreach() {
  declare CMD
  if test 1 -eq $#; then
    CMD="$1"
    declare IPS=()
    for I in $(seq 0 $((COLO_RES_N-1))); do
      IPS+=( "${COLO_RES_IP[$I]}" )
    done
    set "${IPS[@]}" "$CMD"
  fi
  CMD="${*: -1}"
  for I in $(seq 0 $(($#-2))); do
    declare IP="$1"
    colo_instance_run "$IP" "$CMD" &
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
  done < <(colo_instance_run_foreach "[ -n \"\$SOLANA_USER\" ] && echo \"\$SOLANA_USER\"")
  echo "$ME"
}

COLO_SOLANA_USER=""
colo_get_solana_user() {
  if [ -z "$COLO_SOLANA_USER" ]; then
    COLO_SOLANA_USER=$(colo_whoami)
  fi
  echo "$COLO_SOLANA_USER"
}

__colo_node_status_script() {
  cat <<EOF
  exec 3>&2
  exec 2>/dev/null  # Suppress stderr as the next call to exec fails most of
                    # the time due to $SOLANA_LOCK_FILE not existing and is running from a
                    # subshell where normal redirection doesn't work
  exec 9<"$SOLANA_LOCK_FILE" && flock -s 9 && . "$SOLANA_LOCK_FILE" && exec 9>&-
  echo -e "\$SOLANA_LOCK_USER\\v\$SOLANA_LOCK_INSTANCENAME"
  exec 2>&3 # Restore stderr
EOF
}

__colo_node_status_result_normalize() {
  declare IP RC US BY INSTNAME
  declare ST="DOWN"
  IFS=$'\v' read -r IP RC US INSTNAME <<< "$1"
  if [ "$RC" -eq 0 ]; then
    if [ -n "$US" ]; then
      BY="$US"
      ST="HELD"
      if [[ -z "$INSTNAME" ]]; then
        return
      fi
    else
      ST="FREE"
    fi
  fi
  echo -e $"$IP\v$ST\v$BY\v$INSTNAME"
}

colo_node_status() {
  declare IP="$1"
  __colo_node_status_result_normalize "$(colo_instance_run "$IP" "$(__colo_node_status_script)")"
}

colo_node_status_all() {
  declare LINE
  while read -r LINE; do
    __colo_node_status_result_normalize "$LINE"
  done < <(colo_instance_run_foreach "$(__colo_node_status_script)")
}

# TODO: As part of COLO_TOOD_PARALLELIZE this list will need to be maintained
# in a lockfile to work around `cloud_CreateInstance` being called in the
# background for fullnodes
export COLO_RES_REQUISITIONED=()
colo_node_requisition() {
  declare IP=$1
  # shellcheck disable=SC2034
  declare INSTANCE_NAME=$2
  # shellcheck disable=SC2034
  declare SSH_PRIVATE_KEY="$3"

  declare INDEX
  INDEX=$(colo_res_index_from_ip "$IP")
  declare RC=false

  colo_instance_run "$IP" "$(eval "cat <<EOF
  $(<"$__colo_here"/colo-node-onacquire-sh)
EOF
  ")"
  # shellcheck disable=SC2181
  if [[ 0 -eq $? ]]; then
    COLO_RES_REQUISITIONED+=("$INDEX")
    RC=true
  fi
  $RC
}

colo_node_is_requisitioned() {
  declare INDEX="$1"
  declare REQ
  declare RC=false
  for REQ in "${COLO_RES_REQUISITIONED[@]}"; do
    if [[ $REQ -eq $INDEX ]]; then
      RC=true
      break
    fi
  done
  $RC
}

colo_machine_types_compatible() {
  declare MAYBE_MACH="$1"
  declare WANT_MACH="$2"
  declare COMPATIBLE=false
  # XXX: Colo machine types are just GPU count ATM...
  if [[ "$MAYBE_MACH" -ge "$WANT_MACH" ]]; then
    COMPATIBLE=true
  fi
  $COMPATIBLE
}

colo_node_free() {
  declare IP=$1
  colo_instance_run "$IP" "$(eval "cat <<EOF
  $(<"$__colo_here"/colo-node-onfree-sh)
EOF
  ")"
}


