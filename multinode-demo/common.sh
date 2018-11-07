# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

rsync=rsync
leader_logger="tee leader.log"
validator_logger="tee validator.log"
drone_logger="tee drone.log"

if [[ $(uname) != Linux ]]; then
  # Protect against unsupported configurations to prevent non-obvious errors
  # later. Arguably these should be fatal errors but for now prefer tolerance.
  if [[ -n $USE_SNAP ]]; then
    echo "Warning: Snap is not supported on $(uname)"
    USE_SNAP=
  fi
  if [[ -n $SOLANA_CUDA ]]; then
    echo "Warning: CUDA is not supported on $(uname)"
    SOLANA_CUDA=
  fi
fi

if [[ -d $SNAP ]]; then # Running inside a Linux Snap?
  solana_program() {
    declare program="$1"
    printf "%s/command-%s.wrapper" "$SNAP" "$program"
  }
  rsync="$SNAP"/bin/rsync
  multilog="$SNAP/bin/multilog t s16777215 n200"
  leader_logger="$multilog $SNAP_DATA/leader"
  validator_logger="$multilog t $SNAP_DATA/validator"
  drone_logger="$multilog $SNAP_DATA/drone"
  # Create log directories manually to prevent multilog from creating them as
  # 0700
  mkdir -p "$SNAP_DATA"/{drone,leader,validator}

elif [[ -n $USE_SNAP ]]; then # Use the Linux Snap binaries
  solana_program() {
    declare program="$1"
    printf "solana.%s" "$program"
  }
elif [[ -n $USE_INSTALL ]]; then # Assume |cargo install| was run
  solana_program() {
    declare program="$1"
    printf "solana-%s" "$program"
  }
else
  solana_program() {
    declare program="$1"
    declare features=""
    if [[ "$program" =~ ^(.*)-cuda$ ]]; then
      program=${BASH_REMATCH[1]}
      features="--features=cuda"
    fi
    if [[ -z $DEBUG ]]; then
      maybe_release=--release
    fi
    printf "cargo run $maybe_release --bin solana-%s %s -- " "$program" "$features"
  }
  if [[ -n $SOLANA_CUDA ]]; then
    # shellcheck disable=2154 # 'here' is referenced but not assigned
    if [[ -z $here ]]; then
      echo "|here| is not defined"
      exit 1
    fi

    # Locate perf libs downloaded by |./fetch-perf-libs.sh|
    LD_LIBRARY_PATH=$(cd "$here" && dirname "$PWD"/target/perf-libs):$LD_LIBRARY_PATH
    export LD_LIBRARY_PATH
  fi
fi

solana_bench_tps=$(solana_program bench-tps)
solana_wallet=$(solana_program wallet)
solana_drone=$(solana_program drone)
solana_fullnode=$(solana_program fullnode)
solana_fullnode_config=$(solana_program fullnode-config)
solana_fullnode_cuda=$(solana_program fullnode-cuda)
solana_genesis=$(solana_program genesis)
solana_keygen=$(solana_program keygen)
solana_ledger_tool=$(solana_program ledger-tool)

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1

# shellcheck source=scripts/configure-metrics.sh
source "$(dirname "${BASH_SOURCE[0]}")"/../scripts/configure-metrics.sh

tune_networking() {
  # Skip in CI
  [[ -z $CI ]] || return 0

  # Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
  if [[ $(uname) = Linux ]]; then
    (
      set -x +e
      # test the existence of the sysctls before trying to set them
      # go ahead and return true and don't exit if these calls fail
      sysctl net.core.rmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_max=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.rmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_default=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_max=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_default=1610612736 1>/dev/null 2>/dev/null
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


SOLANA_CONFIG_DIR=${SNAP_DATA:-$PWD}/config
SOLANA_CONFIG_PRIVATE_DIR=${SNAP_DATA:-$PWD}/config-private
SOLANA_CONFIG_VALIDATOR_DIR=${SNAP_DATA:-$PWD}/config-validator
SOLANA_CONFIG_CLIENT_DIR=${SNAP_USER_DATA:-$PWD}/config-client

rsync_url() { # adds the 'rsync://` prefix to URLs that need it
  declare url="$1"

  if [[ $url =~ ^.*:.*$ ]]; then
    # assume remote-shell transport when colon is present, use $url unmodified
    echo "$url"
    return 0
  fi

  if [[ -d $url ]]; then
    # assume local directory if $url is a valid directory, use $url unmodified
    echo "$url"
    return 0
  fi

  # Default to rsync:// URL
  echo "rsync://$url"
}

# called from drone, validator, client
find_leader() {
  declare leader leader_address
  declare shift=0

  if [[ -d $SNAP ]]; then
    if [[ -n $1 ]]; then
      usage "Error: unexpected parameter: $1"
    fi

    # Select leader from the Snap configuration
    leader_ip=$(snapctl get leader-ip)
    if [[ -z $leader_ip ]]; then
      leader=testnet.solana.com
      leader_ip=$(dig +short "${leader%:*}" | head -n1)
      if [[ -z $leader_ip ]]; then
          usage "Error: unable to resolve IP address for $leader"
      fi
    fi
    leader=$leader_ip
    leader_address=$leader_ip:8001
  else
    if [[ -z $1 ]]; then
      leader=${here}/..        # Default to local tree for rsync
      leader_address=127.0.0.1:8001 # Default to local leader
    elif [[ -z $2 ]]; then
      leader=$1

      declare leader_ip
      leader_ip=$(dig +short "${leader%:*}" | head -n1)

      if [[ -z $leader_ip ]]; then
          usage "Error: unable to resolve IP address for $leader"
      fi

      leader_address=$leader_ip:8001
      shift=1
    else
      leader=$1
      leader_address=$2
      shift=2
    fi
  fi

  echo "$leader" "$leader_address" "$shift"
}
