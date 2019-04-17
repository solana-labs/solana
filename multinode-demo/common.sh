# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

solana_root="$(dirname "${BASH_SOURCE[0]}")/.."

rsync=rsync
bootstrap_leader_logger="tee bootstrap-leader.log"
fullnode_logger="tee fullnode.log"
drone_logger="tee drone.log"

if [[ $(uname) != Linux ]]; then
  # Protect against unsupported configurations to prevent non-obvious errors
  # later. Arguably these should be fatal errors but for now prefer tolerance.
  if [[ -n $SOLANA_CUDA ]]; then
    echo "Warning: CUDA is not supported on $(uname)"
    SOLANA_CUDA=
  fi
fi


if [[ -n $USE_INSTALL || ! -f "$solana_root"/Cargo.toml ]]; then
  solana_program() {
    declare program="$1"
    printf "solana-%s" "$program"
  }
else
  solana_program() {
    declare program="$1"
    declare features="--features="
    if [[ "$program" =~ ^(.*)-cuda$ ]]; then
      program=${BASH_REMATCH[1]}
      features+="cuda,"
    fi

    if [[ -r "$solana_root"/"$program"/Cargo.toml ]]; then
      maybe_package="--package solana-$program"
    fi
    if [[ -n $NDEBUG ]]; then
      maybe_release=--release
    fi
    declare manifest_path="--manifest-path=$solana_root/$program/Cargo.toml"
    printf "cargo run $manifest_path $maybe_release $maybe_package --bin solana-%s %s -- " "$program" "$features"
  }
  # shellcheck disable=2154 # 'here' is referenced but not assigned
  LD_LIBRARY_PATH=$(cd "$here/../target/perf-libs" && pwd):$LD_LIBRARY_PATH
  export LD_LIBRARY_PATH
fi

solana_bench_tps=$(solana_program bench-tps)
solana_drone=$(solana_program drone)
solana_fullnode=$(solana_program fullnode)
solana_fullnode_cuda=$(solana_program fullnode-cuda)
solana_genesis=$(solana_program genesis)
solana_gossip=$(solana_program gossip)
solana_keygen=$(solana_program keygen)
solana_ledger_tool=$(solana_program ledger-tool)
solana_wallet=$(solana_program wallet)

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1

# shellcheck source=scripts/configure-metrics.sh
source "$solana_root"/scripts/configure-metrics.sh

tune_system() {
  # Skip in CI
  [[ -z $CI ]] || return 0

  # shellcheck source=scripts/ulimit-n.sh
  source "$solana_root"/scripts/ulimit-n.sh

  # Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
  if [[ $(uname) = Linux ]]; then
    (
      set -x +e
      # test the existence of the sysctls before trying to set them
      # go ahead and return true and don't exit if these calls fail
      sysctl net.core.rmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_max=161061273 1>/dev/null 2>/dev/null

      sysctl net.core.rmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_default=161061273 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_max=161061273 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_default=161061273 1>/dev/null 2>/dev/null
    ) || true
  fi

  if [[ $(uname) = Darwin ]]; then
    (
      if [[ $(sysctl net.inet.udp.maxdgram | cut -d\  -f2) != 65535 ]]; then
        echo "Adjusting maxdgram to allow for large UDP packets, see BLOB_SIZE in src/packet.rs:"
        set -x
        sudo sysctl net.inet.udp.maxdgram=65535
      fi
    )

  fi
}

airdrop() {
  declare keypair_file=$1
  declare host=$2
  declare amount=$3

  declare address
  address=$($solana_wallet --keypair "$keypair_file" address)

  # TODO: Until https://github.com/solana-labs/solana/issues/2355 is resolved
  # a fullnode needs N lamports as its vote account gets re-created on every
  # node restart, costing it lamports
  declare retries=5

  while ! $solana_wallet --keypair "$keypair_file" --host "$host" airdrop "$amount"; do

    # TODO: Consider moving this retry logic into `solana-wallet airdrop`
    #   itself, currently it does not retry on "Connection refused" errors.
    ((retries--))
    if [[ $retries -le 0 ]]; then
        echo "Airdrop to $address failed."
        return 1
    fi
    echo "Airdrop to $address failed. Remaining retries: $retries"
    sleep 1
  done

  return 0
}

setup_vote_account() {
  declare drone_address=$1
  declare node_id_path=$2
  declare vote_id_path=$3
  declare stake=$4

  declare node_id
  node_id=$($solana_wallet --keypair "$node_id_path" address)

  declare vote_id
  vote_id=$($solana_wallet --keypair "$vote_id_path" address)

  if [[ -f "$vote_id_path".configured ]]; then
    echo "Vote account has already been configured"
  else
    airdrop "$node_id_path" "$drone_address" "$stake" || return $?

    # Fund the vote account from the node, with the node as the node_id
    $solana_wallet --keypair "$node_id_path" --host "$drone_address" \
      create-vote-account "$vote_id" "$node_id" $((stake - 1)) || return $?
  fi

  $solana_wallet --keypair "$node_id_path" --host "$drone_address" show-vote-account "$vote_id"
  return 0
}

fullnode_usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF
usage: $0 [-x] [--blockstream PATH] [--init-complete-file FILE] [--stake LAMPORTS] [--no-voting] [--rpc-port port] [rsync network path to bootstrap leader configuration] [network entry point]

Start a full node on the specified network

  -x                        - start a new, dynamically-configured full node. Does not apply to the bootstrap leader
  -X [label]                - start or restart a dynamically-configured full node with
                              the specified label. Does not apply to the bootstrap leader
  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --stake LAMPORTS          - Number of lamports to stake
  --public-address          - advertise public machine address in gossip.  By default the local machine address is advertised
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node

EOF
  exit 1
}

# The directory on the bootstrap leader that is rsynced by other full nodes as
# they boot (TODO: Eventually this should go away)
SOLANA_RSYNC_CONFIG_DIR=$PWD/config

# Configuration that remains local
SOLANA_CONFIG_DIR=$PWD/config-local
