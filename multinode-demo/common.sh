# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

# shellcheck source=net/common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. || exit 1; pwd)"/net/common.sh

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
    if [[ -z $program ]]; then
      printf "solana"
    else
      printf "solana-%s" "$program"
    fi
  }
else
  solana_program() {
    declare program="$1"
    declare crate="$program"
    if [[ -z $program ]]; then
      crate="cli"
      program="solana"
    else
      program="solana-$program"
    fi

    if [[ -r "$SOLANA_ROOT/$crate"/Cargo.toml ]]; then
      maybe_package="--package solana-$crate"
    fi
    if [[ -n $NDEBUG ]]; then
      maybe_release=--release
    fi
    declare manifest_path="--manifest-path=$SOLANA_ROOT/$crate/Cargo.toml"
    printf "cargo $CARGO_TOOLCHAIN run $manifest_path $maybe_release $maybe_package --bin %s %s -- " "$program"
  }
fi

solana_bench_tps=$(solana_program bench-tps)
solana_drone=$(solana_program drone)
solana_validator=$(solana_program validator)
solana_validator_cuda="$solana_validator --cuda"
solana_genesis=$(solana_program genesis)
solana_gossip=$(solana_program gossip)
solana_keygen=$(solana_program keygen)
solana_ledger_tool=$(solana_program ledger-tool)
solana_cli=$(solana_program)
solana_archiver=$(solana_program archiver)

export RUST_BACKTRACE=1

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
