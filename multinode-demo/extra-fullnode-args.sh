# |source| this file
#
# handle arguments for bootstrap leader and validator fullnodes
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

if [[ $1 = -h ]]; then
  fullnode_usage "$@"
fi

extra_fullnode_args=()

bootstrap_leader=false
stake=43 # number of lamports to assign as stake (plus transaction fee to setup the stake)
poll_for_new_genesis_block=0
label=

while [[ ${1:0:1} = - ]]; do
  if [[ $1 = --label ]]; then
    label="-$2"
    shift 2
  elif [[ $1 = --bootstrap-leader ]]; then
    bootstrap_leader=true
    shift
  elif [[ $1 = --poll-for-new-genesis-block ]]; then
    poll_for_new_genesis_block=1
    shift
  elif [[ $1 = --blockstream ]]; then
    stake=0
    extra_fullnode_args+=("$1" "$2")
    shift 2
  elif [[ $1 = --enable-rpc-exit ]]; then
    extra_fullnode_args+=("$1")
    shift
  elif [[ $1 = --init-complete-file ]]; then
    extra_fullnode_args+=("$1" "$2")
    shift 2
  elif [[ $1 = --stake ]]; then
    stake="$2"
    shift 2
  elif [[ $1 = --no-voting ]]; then
    extra_fullnode_args+=("$1")
    shift
  elif [[ $1 = --no-sigverify ]]; then
    extra_fullnode_args+=("$1")
    shift
  elif [[ $1 = --rpc-port ]]; then
    extra_fullnode_args+=("$1" "$2")
    shift 2
  elif [[ $1 = --dynamic-port-range ]]; then
    extra_fullnode_args+=("$1" "$2")
    shift 2
  elif [[ $1 = --gossip-port ]]; then
    extra_fullnode_args+=("$1" "$2")
    shift 2
  else
    echo "Unknown argument: $1"
    exit 1
  fi
done

if [[ -n $3 ]]; then
  fullnode_usage "$@"
fi

default_fullnode_arg() {
  declare name=$1
  declare value=$2

  for arg in "${extra_fullnode_args[@]}"; do
    if [[ $arg = "$name" ]]; then
      return
    fi
  done

  if [[ -n $value ]]; then
    extra_fullnode_args+=("$name" "$value")
  else
    extra_fullnode_args+=("$name")
  fi
}
