#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=net/common.sh
source "$here"/common.sh

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 source ... target

node scp - behaves like regular scp with the necessary options to
access network nodes added automatically

EOF
  exit $exitcode
}

while getopts "h?" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

loadConfigFile

if [[ -n "$1" ]]; then
  set -x
  exec scp "${sshOptions[@]}" "$@"
fi

exec "$here"/ssh.sh

exit 0
