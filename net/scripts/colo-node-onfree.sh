#!/usr/bin/env bash

# These variable must be set before the main body is called
SOLANA_LOCK_FILE="${SOLANA_LOCK_FILE:?}"
SECONDARY_DISK_MOUNT_POINT="${SECONDARY_DISK_MOUNT_POINT:?}"
SSH_AUTHORIZED_KEYS="${SSH_AUTHORIZED_KEYS:?}"
FORCE_DELETE="${FORCE_DELETE:?}"

RC=false
if [[ -f "${SOLANA_LOCK_FILE}" ]]; then
  exec 9<>"${SOLANA_LOCK_FILE}"
  flock -x -n 9 || ( echo "Failed to acquire lock!" 1>&2 && exit 1 )
  # shellcheck disable=SC1090
  . "${SOLANA_LOCK_FILE}"
  if [[ "${SOLANA_LOCK_USER}" = "${SOLANA_USER}"  || -n "${FORCE_DELETE}" ]]; then
    # Begin running process cleanup
    CLEANUP_PID=$$
    CLEANUP_PIDS=()
    CLEANUP_PPIDS=()
    get_pids() {
      CLEANUP_PIDS=()
      CLEANUP_PPIDS=()
      declare line maybe_ppid maybe_pid
      while read -r line; do
        read -r maybe_ppid maybe_pid _ _ _ _ _ _ _ _ <<<"${line}"
        CLEANUP_PIDS+=( "${maybe_pid}" )
        CLEANUP_PPIDS+=( "${maybe_ppid}" )
      done < <(ps jxh | sort -rn -k2,2)
    }

    CLEANUP_PROC_CHAINS=()
    resolve_chains() {
      CLEANUP_PROC_CHAINS=()
      declare i pid ppid handled n
      for i in "${!CLEANUP_PIDS[@]}"; do
        pid=${CLEANUP_PIDS[${i}]}
        ppid=${CLEANUP_PPIDS[${i}]}
        handled=false

        for j in "${!CLEANUP_PROC_CHAINS[@]}"; do
          if grep -q "^${ppid}\b" <<<"${CLEANUP_PROC_CHAINS[${j}]}"; then
            CLEANUP_PROC_CHAINS[${j}]="${pid} ${CLEANUP_PROC_CHAINS[${j}]}"
            handled=true
            break
          elif grep -q "\b${pid}\$" <<<"${CLEANUP_PROC_CHAINS[${j}]}"; then
            CLEANUP_PROC_CHAINS[${j}]+=" ${ppid}"
            handled=true
            # Don't break, we may be the parent of may proc chains
          fi
        done
        if ! ${handled}; then
          n=${#CLEANUP_PROC_CHAINS[@]}
          CLEANUP_PROC_CHAINS[${n}]="${pid} ${ppid}"
        fi
      done
    }

    # Kill screen sessions
    while read -r SID; do
      screen -S "${SID}" -X quit
    done < <(screen -wipe 2>&1 | sed -e 's/^\s\+\([^[:space:]]\+\)\s.*/\1/;t;d')

    # Kill tmux sessions
    tmux kill-server &> /dev/null

    # Kill other processes
    for SIG in INT TERM KILL; do
      get_pids
      if [[ ${#CLEANUP_PIDS[@]} -eq 0 ]]; then
        break
      else
        resolve_chains
        for p in "${CLEANUP_PROC_CHAINS[@]}"; do
          if ! grep -q "\b${CLEANUP_PID}\b" <<<"${p}"; then
            read -ra TO_KILL <<<"${p}"
            N=${#TO_KILL[@]}
            ROOT_PPID="${TO_KILL[$((N-1))]}"
            if [[ 1 -ne ${ROOT_PPID} ]]; then
              LAST_PID_IDX=$((N-2))
              for I in $(seq 0 ${LAST_PID_IDX}); do
                pid="${TO_KILL[${I}]}"
                kill "-${SIG}" "${pid}" &>/dev/null
              done
            fi
          fi
        done
        get_pids
        if [[ ${#CLEANUP_PIDS[@]} -gt 0 ]]; then
          sleep 5
        fi
      fi
    done
    # End running process cleanup

    # Begin filesystem cleanup
    git clean -qxdff
    rm -f /solana-scratch/* /solana-scratch/.[^.]*
    cat > "${HOME}/.ssh/authorized_keys" <<EOAK
${SSH_AUTHORIZED_KEYS}
EOAK
    EXTERNAL_CONFIG_DIR="${SECONDARY_DISK_MOUNT_POINT}/config/"
    if [[ -d "${EXTERNAL_CONFIG_DIR}" ]]; then
      rm -rf "${EXTERNAL_CONFIG_DIR}"
    fi
    # End filesystem cleanup
    RC=true
  else
    echo "Invalid user: expected \"${SOLANA_LOCK_USER}\" got \"${SOLANA_USER}\"" 1>&2
  fi
  exec 9>&-
fi
${RC}

