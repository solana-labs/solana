# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

SOLANA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. || exit 1; pwd)"

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

if [[ -n $USE_INSTALL || ! -f "$SOLANA_ROOT"/Cargo.toml ]]; then
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

    if [[ -r "$SOLANA_ROOT/$program"/Cargo.toml ]]; then
      maybe_package="--package solana-$program"
    fi
    if [[ -n $NDEBUG ]]; then
      maybe_release=--release
    fi
    declare manifest_path="--manifest-path=$SOLANA_ROOT/$program/Cargo.toml"
    printf "cargo run $manifest_path $maybe_release $maybe_package --bin solana-%s %s -- " "$program" "$features"
  }
  # shellcheck disable=2154 # 'here' is referenced but not assigned
  LD_LIBRARY_PATH=$(cd "$SOLANA_ROOT/target/perf-libs" && pwd):$LD_LIBRARY_PATH
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
source "$SOLANA_ROOT"/scripts/configure-metrics.sh

tune_system() {
  # Skip in CI
  [[ -z $CI ]] || return 0

  # shellcheck source=scripts/ulimit-n.sh
  source "$SOLANA_ROOT"/scripts/ulimit-n.sh

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

fullnode_usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF
usage: $0 [--blockstream PATH] [--init-complete-file FILE] [--label LABEL] [--stake LAMPORTS] [--no-voting] [--rpc-port port] [rsync network path to bootstrap leader configuration] [network entry point]

Start a full node

  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the fullnode configuration files, useful when running
                              multiple fullnodes from the same filesystem location
  --stake LAMPORTS          - Number of lamports to stake
  --public-address          - advertise public machine address in gossip.  By default the local machine address is advertised
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node

EOF
  exit 1
}

# The directory on the cluster entrypoint that is rsynced by other full nodes
SOLANA_RSYNC_CONFIG_DIR=$SOLANA_ROOT/config

# Configuration that remains local
SOLANA_CONFIG_DIR=$SOLANA_ROOT/config-local
