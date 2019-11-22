#!/usr/bin/env bash

# These variable must be set before the main body is called
SOLANA_LOCK_FILE="${SOLANA_LOCK_FILE:?}"
INSTANCE_NAME="${INSTANCE_NAME:?}"
SSH_AUTHORIZED_KEYS="${SSH_AUTHORIZED_KEYS:?}"
SSH_PRIVATE_KEY_TEXT="${SSH_PRIVATE_KEY_TEXT:?}"
SSH_PUBLIC_KEY_TEXT="${SSH_PUBLIC_KEY_TEXT:?}"
NETWORK_INFO="${NETWORK_INFO:-"Network info unavailable"}"
CREATION_INFO="${CREATION_INFO:-"Creation info unavailable"}"

if [[ ! -f "${SOLANA_LOCK_FILE}" ]]; then
  exec 9>>"${SOLANA_LOCK_FILE}"
  flock -x -n 9 || ( echo "Failed to acquire lock!" 1>&2 && exit 1 )
  SOLANA_USER="${SOLANA_USER:?"SOLANA_USER undefined"}"
  {
    echo "export SOLANA_LOCK_USER=${SOLANA_USER}"
    echo "export SOLANA_LOCK_INSTANCENAME=${INSTANCE_NAME}"
    echo "[[ -v SSH_TTY -a -f \"${HOME}/.solana-motd\" ]] && cat \"${HOME}/.solana-motd\" 1>&2"
  } >&9
  exec 9>&-
  cat > /solana-scratch/id_ecdsa <<EOF
${SSH_PRIVATE_KEY_TEXT}
EOF
  cat > /solana-scratch/id_ecdsa.pub <<EOF
${SSH_PUBLIC_KEY_TEXT}
EOF
  chmod 0600 /solana-scratch/id_ecdsa
  cat > /solana-scratch/authorized_keys <<EOF
${SSH_AUTHORIZED_KEYS}
${SSH_PUBLIC_KEY_TEXT}
EOF
  cp /solana-scratch/id_ecdsa "${HOME}/.ssh/id_ecdsa"
  cp /solana-scratch/id_ecdsa.pub "${HOME}/.ssh/id_ecdsa.pub"
  cp /solana-scratch/authorized_keys "${HOME}/.ssh/authorized_keys"
  cat > "${HOME}/.solana-motd" <<EOF


${NETWORK_INFO}
${CREATION_INFO}
EOF

  # Stamp creation MUST be last!
  touch /solana-scratch/.instance-startup-complete
else
  # shellcheck disable=SC1090
  exec 9<"${SOLANA_LOCK_FILE}" && flock -s 9 && . "${SOLANA_LOCK_FILE}" && exec 9>&-
  echo "${INSTANCE_NAME} candidate is already ${SOLANA_LOCK_INSTANCENAME}" 1>&2
  false
fi
