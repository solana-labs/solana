#!/bin/bash

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
usage: $0 [ipAddress] [extra ssh arguments]

ssh into a node

 ipAddress     - IP address of the desired node.

If ipAddress is unspecified, a list of available nodes will be displayed.

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

ipAddress=$1
shift
if [[ -n "$ipAddress" ]]; then
  set -x
  exec ssh "${sshOptions[@]}" "$ipAddress" "$@"
fi


echo Leader:
echo -e "  $0 $leaderIp\t|  $0 $leaderIp tail -f solana/leader.log"
echo
echo Validators:
for ipAddress in "${validatorIpList[@]}"; do
  echo -e "  $0 $ipAddress\t|  $0 $ipAddress tail -f solana/validator.log"
done
echo
echo Clients:
if [[ ${#clientIpList[@]} -eq 0 ]]; then
  echo "  None"
else
  for ipAddress in "${clientIpList[@]}"; do
    echo -e "  $0 $ipAddress\t|  $0 $ipAddress tail -f solana/client.log"
  done
fi

exit 0
