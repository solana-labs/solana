#!/usr/bin/env bash
set -e

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
usage: $0 [-e] [-d] [username]

Creates a testnet dev metrics database

  username        InfluxDB user with access to create a new database
  -d              Delete the database instead of creating it
  -e              Assume database already exists and SOLANA_METRICS_CONFIG is
                  defined in the environment already

EOF
  exit $exitcode
}

loadConfigFile

useEnv=false
delete=false
host="https://metrics.solana.com:8086"
while getopts "hde" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  d)
    delete=true
    ;;
  e)
    useEnv=true
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done
shift $((OPTIND - 1))

if $useEnv; then
  [[ -n $SOLANA_METRICS_CONFIG ]] ||
    usage "SOLANA_METRICS_CONFIG is not defined in the environment"
else
  username=$1
  [[ -n "$username" ]] || usage "username not specified"

  read -rs -p "InfluxDB password for $username: " password
  [[ -n $password ]] || { echo "Password not specified"; exit 1; }
  echo

  query() {
    echo "$*"
    curl -XPOST \
      "$host/query?u=${username}&p=${password}" \
      --data-urlencode "q=$*"
  }

  query "DROP DATABASE \"$netBasename\""
  ! $delete || exit 0
  query "CREATE DATABASE \"$netBasename\""
  query "ALTER RETENTION POLICY autogen ON \"$netBasename\" DURATION 7d"
  query "GRANT READ ON \"$netBasename\" TO \"ro\""
  query "GRANT WRITE ON \"$netBasename\" TO \"scratch_writer\""

  SOLANA_METRICS_CONFIG="host=$host,db=$netBasename,u=scratch_writer,p=topsecret"
fi

echo "export SOLANA_METRICS_CONFIG=\"$SOLANA_METRICS_CONFIG\"" >> "$configFile"

exit 0
