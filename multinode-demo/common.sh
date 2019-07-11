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

if [[ $(uname) != Linux ]]; then
  # Protect against unsupported configurations to prevent non-obvious errors
  # later. Arguably these should be fatal errors but for now prefer tolerance.
  if [[ -n $SOLANA_CUDA ]]; then
    echo "Warning: CUDA is not supported on $(uname)"
    SOLANA_CUDA=
  fi
fi

if [[ -f "$SOLANA_ROOT"/target/perf-libs/env.sh ]]; then
  # shellcheck source=/dev/null
  source "$SOLANA_ROOT"/target/perf-libs/env.sh
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
      features+="cuda"
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
fi

solana_bench_tps=$(solana_program bench-tps)
solana_drone=$(solana_program drone)
solana_validator=$(solana_program validator)
solana_validator_cuda=$(solana_program validator-cuda)
solana_genesis=$(solana_program genesis)
solana_gossip=$(solana_program gossip)
solana_keygen=$(solana_program keygen)
solana_ledger_tool=$(solana_program ledger-tool)
solana_wallet=$(solana_program wallet)
solana_replicator=$(solana_program replicator)

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1

# shellcheck source=scripts/configure-metrics.sh
source "$SOLANA_ROOT"/scripts/configure-metrics.sh

# The directory on the cluster entrypoint that is rsynced by other full nodes
SOLANA_RSYNC_CONFIG_DIR=$SOLANA_ROOT/config

# Configuration that remains local
SECONDARY_DISK_MOUNT_POINT=/mnt/extra-disk
if [[ -d $SECONDARY_DISK_MOUNT_POINT ]]; then
  SOLANA_CONFIG_DIR=$SECONDARY_DISK_MOUNT_POINT/config-local
else
  SOLANA_CONFIG_DIR=$SOLANA_ROOT/config-local
fi

default_arg() {
  declare name=$1
  declare value=$2

  for arg in "${args[@]}"; do
    if [[ $arg = "$name" ]]; then
      return
    fi
  done

  if [[ -n $value ]]; then
    args+=("$name" "$value")
  else
    args+=("$name")
  fi
}

replace_arg() {
  declare name=$1
  declare value=$2

  default_arg "$name" "$value"

  declare index=0
  for arg in "${args[@]}"; do
    index=$((index + 1))
    if [[ $arg = "$name" ]]; then
      args[$index]="$value"
    fi
  done
}
