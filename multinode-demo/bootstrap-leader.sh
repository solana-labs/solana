#!/usr/bin/env bash
#
# Start the bootstrap leader node
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

if [[ -d "$SNAP" ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n "$(snapctl get mode)" ]] || exit 0
fi

[[ -f "$SOLANA_CONFIG_DIR"/bootstrap-leader.json ]] || {
  echo "$SOLANA_CONFIG_DIR/bootstrap-leader.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

if [[ -n "$SOLANA_CUDA" ]]; then
  program="$solana_fullnode_cuda"
else
  program="$solana_fullnode"
fi

maybe_no_leader_rotation=
if [[ $1 = --no-leader-rotation ]]; then
  maybe_no_leader_rotation="--no-leader-rotation"
  shift
fi

if [[ -n $1 ]]; then
  echo "Unknown argument: $1"
  exit 1
fi

if [[ -d $SNAP ]]; then
  if [[ $(snapctl get leader-rotation) = false ]]; then
    maybe_no_leader_rotation="--no-leader-rotation"
  fi
fi

tune_system

trap 'kill "$pid" && wait "$pid"' INT TERM
$solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify
$program \
  $maybe_no_leader_rotation \
  --identity "$SOLANA_CONFIG_DIR"/bootstrap-leader.json \
  --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger \
  --rpc-port 8899 \
  > >($bootstrap_leader_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
