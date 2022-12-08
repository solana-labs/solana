#!/usr/bin/env bash

# need to setup manually
NODE_POOL=()

DEFAULT_CLIENT_COUNT=1
DEFAULT_VALIDATOR_COUNT=2

IN_USE_CHECK_FILE=/home/solana/.solana.lock

usage() {
  cat <<EOF

USAGE:
  $0 [SUBCOMMAND] [OPTIONS]

SUBCOMMAND:
  create                      - allocate nodes from colo pool
  delete                      - deallocate nodes
  status                      - display status information of all nodes
  add-pool-to-known-hosts     - add pool to known hoot

OPTIONS:
  -h, --help        - prints help information

EOF
}

usage_create() {
  cat <<EOF

USAGE:
  $0 create [OPTIONS] -k <SSH_PRIVATE_KEY_PATH>

OPTIONS:
  -n <NUMBER>      - number of additional validators (default: $DEFAULT_VALIDATOR_COUNT)
  -c <NUMBER>      - number of client nodes (default: $DEFAULT_CLIENT_COUNT)

EOF
}

usage_common() {
  cat <<EOF

USAGE:
  $0 $1 [OPTIONS]

OPTIONS:
  -k <SSH_PRIVATE_KEY_PATH>    - private key file path

EOF
}

ssh_options=(
  -o "StrictHostKeyChecking=no"
  -o "ConnectTimeout=10"
  -o "UserKnownHostsFile=/dev/null"
  -o "User=solana"
  -o "LogLevel=ERROR"
)

private_key_path=
parse_global_options() {
  remain_args=()

  while [ "${#ARGS[@]}" -gt 0 ]; do
    arg=
    array_pop ARGS arg
    case $arg in
    -k)
      array_pop ARGS private_key_path || return 1
      ssh_options+=(-o "IdentityFile=$private_key_path")
      ;;
    -h | --h | -help | --help | help) return 1 ;;
    *) remain_args+=("$arg") ;;
    esac
  done

  ARGS=("${remain_args[@]}")
}

validator_count=$DEFAULT_VALIDATOR_COUNT
client_count=$DEFAULT_CLIENT_COUNT
parse_create_options() {
  remain_args=()

  while [ "${#ARGS[@]}" -gt 0 ]; do
    arg=
    array_pop ARGS arg
    case $arg in
    -n) array_pop ARGS validator_count || return 1 ;;
    -c) array_pop ARGS client_count || return 1 ;;
    *) remain_args+=("$arg") ;;
    esac
  done

  ARGS=("${remain_args[@]}")
}

create() {
  if ! parse_global_options; then
    usage_create
    return 1
  fi
  if ! parse_create_options; then
    usage_create
    return 1
  fi

  # check if private_key_path is empty
  if [ -z "$private_key_path" ]; then
    echo "please use \`-k <KEY_PATH>\` to specify the private key path"
    return 1
  fi

  echo "request validator: $validator_count, client: $client_count"

  need=$((validator_count + client_count))
  ip_list=()
  for ip in "${NODE_POOL[@]}"; do
    if [ "$need" -eq 0 ]; then
      break
    fi

    result=$(ssh "${ssh_options[@]}" "$ip" test -f $IN_USE_CHECK_FILE >/dev/null 2>&1)
    exit_code=$?
    if [ "$exit_code" -eq 1 ]; then
      ip_list+=("$ip")
      ((need -= 1))
      echo "check $ip ✅"
    elif [ "$exit_code" -eq 0 ]; then
      echo "check $ip ❌ (in use)"
    else
      echo "check $ip ❌ (${result:-unknown error})"
    fi
  done

  if [ "$need" -ne 0 ]; then
    echo "insufficient nodes"
    return 1
  fi

  for ip in "${ip_list[@]}"; do
    echo "setup $ip"
    if ! ssh "${ssh_options[@]}" "$ip" "set -o noclobber; { > $IN_USE_CHECK_FILE ; } &> /dev/null;"; then
      echo "error: race condition on $ip"
      echo "recover registed nodes... (please don't interrupt this process)"
      _delete ip_list
      return 1
    fi
  done

  # copy all keys to the bootstrap node (the first validator)
  if [ "$validator_count" -gt 0 ]; then
    echo "collect infos..."
    for ip in "${ip_list[@]:1}"; do
      pubkey=$(ssh "${ssh_options[@]}" "$ip" cat /home/solana/.ssh/id_ed25519.pub)
      ssh "${ssh_options[@]}" "${ip_list[0]}" "echo $pubkey >>/home/solana/.ssh/authorized_keys_net"
    done
  fi

  validator_ip_list=("${ip_list[@]:0:$validator_count}")
  client_ip_list=("${ip_list[@]:$validator_count:$((validator_count + client_count))}")
  # generate config file
  mkdir -p config
  cat <<EOF >config/config
  netBasename=testnet-dev-$USER-$(date +%s)
  publicNetwork=true
  sshPrivateKey=$private_key_path
  letsEncryptDomainName=
  export TMPFS_ACCOUNTS=false
  validatorIpList=(${validator_ip_list[@]})
  validatorIpListPrivate=(${validator_ip_list[@]})
  validatorIpListZone=()
  clientIpList=(${client_ip_list[@]})
  clientIpListPrivate=(${client_ip_list[@]})
  clientIpListZone=()
  blockstreamerIpList=()
  blockstreamerIpListPrivate=()
EOF

}

delete() {
  if ! parse_global_options; then
    usage_common
    return 1
  fi

  if [[ ${#ARGS} -eq 0 ]]; then
    if [[ -t "config/config" ]]; then
      echo "can't fine config file"
      return 1
    fi

    source config/config
    _delete validatorIpList
  else
    _delete ARGS
  fi

}

_delete() {
  local -n iplist=$1

  for ip in "${iplist[@]}"; do
    ssh "${ssh_options[@]}" "$ip" &>/dev/null <<"EOF"
    rm /home/solana/.solana.lock
    rm /home/solana/.ssh/authorized_keys_net
    rm /home/solana/version.yml
    rm -rf /home/solana/solana
EOF
    echo "$ip ✅"
  done
}

status() {
  if ! parse_global_options; then
    usage_common status
    return 1
  fi

  for ip in "${NODE_POOL[@]}"; do
    (
      result=$(ssh "${ssh_options[@]}" "$ip" test -f $IN_USE_CHECK_FILE >/dev/null 2>&1)
      exit_code=$?
      if [ "$exit_code" -eq 1 ]; then
        echo "check $ip ✅"
      elif [ "$exit_code" -eq 0 ]; then
        echo "check $ip ❌ (in use)"
      else
        echo "check $ip ❌ (${result:-unknown error})"
      fi
    ) &
  done

  wait
}

add_pool_to_known_hosts() {
  if ! parse_global_options; then
    usage_common add_pool_to_known_hosts
    return 1
  fi

  for ip in "${NODE_POOL[@]}"; do
    ssh-keyscan "$ip" >>~/.ssh/known_hosts 2>&1 >/dev/null
    echo "$ip ✅"
  done
}

check() {
  local ip=$1

  ssh "${ssh_options[@]}" "$ip" test -f $IN_USE_CHECK_FILE >/dev/null 2>&1
}

array_pop() {
  local -n input=$1
  local -n output=$2

  if [[ "${#input[@]}" -lt 1 ]]; then
    return 1
  fi

  # shellcheck disable=SC2034
  output=${input[0]}
  input=("${input[@]:1}")
}

main() {
  # parse command
  cmd=
  if ! array_pop ARGS cmd; then
    usage
    return 1
  fi

  # actions
  case "$cmd" in
  create) create ;;
  delete) delete ;;
  status) status ;;
  add-pool-to-known-hosts) add_pool_to_known_hosts ;;
  *) usage ;;
  esac
}

ARGS=("$@")

main
