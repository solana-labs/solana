#!/usr/bin/env bash

# We need root access, but also appropriate envvar values. Require scripts to
# run with sudo as a normal user
ensure_env() {
  RC=false
  [ $EUID -eq 0 ] && [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ] && RC=true
  if $RC; then
    export SETUP_USER="$SUDO_USER"
    export SETUP_HOME="$HOME"
  else
    echo "Please run \"$0\" via sudo as a normal user"
  fi
  $RC
}

