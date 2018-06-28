# |source| this file
#
# Disable complaints about unused variables in this file:
# shellcheck disable=2034

if [[ -d "$SNAP" ]]; then # Running inside a Linux Snap?
  solana_program() {
    declare program="$1"
    printf "%s/command-%s.wrapper" "$SNAP" "$program"
  }
  SOLANA_CUDA="$(snapctl get enable-cuda)"

elif [[ -n "$USE_SNAP" ]]; then # Use the Linux Snap binaries
  solana_program() {
    declare program="$1"
    printf "solana.%s" "$program"
  }
elif [[ -n "$USE_INSTALL" ]]; then # Assume |cargo install| was run
  solana_program() {
    declare program="$1"
    printf "solana-%s" "$program"
  }
  # CUDA was/wasn't selected at build time, can't affect CUDA state here
  unset SOLANA_CUDA
else
  solana_program() {
    declare program="$1"
    declare features=""
    if [[ "$program" =~ ^(.*)-cuda$ ]]; then
      program=${BASH_REMATCH[1]}
      features="--features=cuda,erasure"
    fi
    if [[ -z "$DEBUG" ]]; then
      maybe_release=--release
    fi
    printf "cargo run $maybe_release --bin solana-%s %s -- " "$program" "$features"
  }
fi

solana_client_demo=$(solana_program client-demo)
solana_wallet=$(solana_program wallet)
solana_drone=$(solana_program drone)
solana_fullnode=$(solana_program fullnode)
solana_fullnode_config=$(solana_program fullnode-config)
solana_fullnode_cuda=$(solana_program fullnode-cuda)
solana_genesis=$(solana_program genesis)
solana_mint=$(solana_program mint)

export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
[[ $(uname) = Linux ]] && (set -x; sudo sysctl -w net.core.rmem_max=26214400 1>/dev/null 2>/dev/null)

SOLANA_CONFIG_DIR=${SNAP_DATA:-$PWD}/config

rsync_url() { # adds the 'rsync://` prefix to URLs that need it
  declare url="$1"

  if [[ "$url" =~ ^.*:.*$ ]]; then
    # assume remote-shell transport when colon is present, use $url unmodified
    echo "$url"
    return
  fi

  if [[ -d "$url" ]]; then
    # assume local directory if $url is a valid directory, use $url unmodified
    echo "$url"
    return
  fi

  # Default to rsync:// URL
  echo "rsync://$url"
}
