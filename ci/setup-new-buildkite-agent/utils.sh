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

# Some scripts disable SSH password logins. If no one hash setup authorized_keys
# this will result in the machine being remotely inaccessible. Check that the
# user running this script has setup their keys
check_ssh_authorized_keys() {
  declare rc=false
  declare user_home=
  if [[ -n "$SUDO_USER" ]]; then
    declare user uid gid home
    declare passwd_entry
    passwd_entry="$(grep "$SUDO_USER:[^:]*:$SUDO_UID:$SUDO_GID" /etc/passwd)"
    IFS=: read -r user _ uid gid _ home _ <<<"$passwd_entry"
    if [[ "$user" == "$SUDO_USER" && "$uid" == "$SUDO_UID" && "$gid" == "$SUDO_GID" ]]; then
      user_home="$home"
    fi
  else
    user_home="$HOME"
  fi
  declare authorized_keys="${user_home}/.ssh/authorized_keys"
  if [[ -n "$user_home" ]]; then
    [[ -s "$authorized_keys" ]] && rc=true
  fi
  if ! $rc; then
    echo "ERROR! This script will disable SSH password logins and you don't"
    echo "appear to have set up any authorized keys.  Please add you SSH"
    echo "public key to ${authorized_keys} before continuing!"
  fi
  $rc
}

check_ssh_authorized_keys
