#!/usr/bin/env bash

set -x
! tmux list-sessions || tmux kill-session
declare sudo=
if sudo true; then
  sudo="sudo -n"
fi

echo "pwd: $(pwd)"
for pid in safecoin/*.pid; do
  pgid=$(ps opgid= "$(cat "$pid")" | tr -d '[:space:]')
  if [[ -n $pgid ]]; then
    $sudo kill -- -"$pgid"
  fi
done
if [[ -f safecoin/netem.cfg ]]; then
  safecoin/scripts/netem.sh delete < safecoin/netem.cfg
  rm -f safecoin/netem.cfg
fi
safecoin/scripts/net-shaper.sh cleanup
for pattern in validator.sh boostrap-leader.sh safecoin- remote- iftop validator client node; do
  echo "killing $pattern"
  pkill -f $pattern
done
