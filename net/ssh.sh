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
usage: $0 [-f configFile] [ipAddress]

ssh into a node

 ipAddress     - IP address of the desired node.
 -f configFile - Configuration file created by ./gce_multinode.sh
                 (default: $configFile)


If ipAddress is unspecified, a list of available nodes will be displayed.

EOF
  exit $exitcode
}

while getopts "h?f:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  f)
    configFile=$OPTARG
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

loadConfigFile

ipAddress=$1
if [[ -n "$ipAddress" ]]; then
  set -x
  exec ssh "${sshOptions[@]}" "$ipAddress"
fi

echo Leader:
echo "  $0 $leaderIp"
echo
echo Validators:
for ipAddress in "${validatorIpList[@]}"; do
  echo "  $0 $ipAddress"
done
echo
echo Clients:
if [[ ${#clientIpList[@]} -eq 0 ]]; then
  echo "  None"
else
  for ipAddress in "${clientIpList[@]}"; do
    echo "  $0 $ipAddress"
  done
fi

exit 0
