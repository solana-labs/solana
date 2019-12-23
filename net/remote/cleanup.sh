#!/usr/bin/env bash

set -x
! tmux list-sessions || tmux kill-session
declare sudo=
if sudo true; then
  sudo="sudo -n"
fi

echo "pwd: $(pwd)"
for pid in solana/*.pid; do
  pgid=$(ps opgid= "$(cat "$pid")" | tr -d '[:space:]')
  if [[ -n $pgid ]]; then
    $sudo kill -- -"$pgid"
  fi
done
if [[ -f solana/netem.cfg ]]; then
  solana/scripts/netem.sh delete < solana/netem.cfg
  rm -f solana/netem.cfg
fi
solana/scripts/net-shaper.sh force_cleanup
for pattern in validator.sh boostrap-leader.sh solana- remote- iftop validator client node; do
  echo "killing $pattern"
  pkill -f $pattern
done
