#!/bin/bash -e

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
usage: $0 [-d] [username]

Creates a testnet dev metrics database

  username        InfluxDB user with access to create a new database
  -d              Delete the database instead of creating it

EOF
  exit $exitcode
}

loadConfigFile

delete=false
while getopts "hd" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  d)
    delete=true
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done
shift $((OPTIND - 1))

username=$1
[[ -n "$username" ]] || usage "username not specified"

read -rs -p "InfluxDB password for $username: " password
[[ -n $password ]] || { echo "Password not specified"; exit 1; }
echo

query() {
  echo "$*"
  curl -XPOST \
    "https://metrics.solana.com:8086/query?u=${username}&p=${password}" \
    --data-urlencode "q=$*"
}

query "DROP DATABASE \"$netBasename\""
! $delete || exit 0
query "CREATE DATABASE \"$netBasename\""
query "ALTER RETENTION POLICY autogen ON \"$netBasename\" DURATION 7d"
query "GRANT READ ON \"$netBasename\" TO \"ro\""
query "GRANT WRITE ON \"$netBasename\" TO \"scratch_writer\""

echo "export \
  SOLANA_METRICS_CONFIG=\"db=$netBasename,u=scratch_writer,p=topsecret\" \
" >> "$configFile"

exit 0
