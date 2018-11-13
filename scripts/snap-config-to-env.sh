#!/usr/bin/env bash
#
# Snap daemons have no access to the environment so |snap set solana ...| is
# used to set runtime configuration.
#
# This script exports the snap runtime configuration options back as
# environment variables before invoking the specified program
#

if [[ -d $SNAP ]]; then # Running inside a Linux Snap?
  RUST_LOG="$(snapctl get rust-log)"
  SOLANA_DEFAULT_METRICS_RATE="$(snapctl get default-metrics-rate)"
  SOLANA_METRICS_CONFIG="$(snapctl get metrics-config)"

  export RUST_LOG
  export SOLANA_DEFAULT_METRICS_RATE
  export SOLANA_METRICS_CONFIG
fi

exec "$@"
