#!/usr/bin/env bash

cd "$(dirname "$0")/../.."
command=$1

case $command in
stop)
  echo --- stop network software
  net/net.sh stop -p $TESTNET_TAG
  ;;
delete)
  echo --- delete testnet
  case $CLOUD_PROVIDER in
    gce)
      net/gce.sh delete -p $TESTNET_TAG
      ;;
    colo)
      net/colo.sh delete -p $TESTNET_TAG
      ;;
    *)
      echo "Error: Unsupported cloud provider: $CLOUD_PROVIDER"
      ;;
    esac
    ;;
logs)
  echo --- collect logs from remote nodes
  rm -rf net/log
  net/net.sh logs
  for logfile in $(ls -A net/log) ; do
    new_log=net/log/"$TESTNET_TAG"_"$NUMBER_OF_VALIDATOR_NODES"-nodes_"$(basename "$logfile")"
    cp net/log/"$logfile" "$new_log"
    upload-ci-artifact "$new_log"
  done
  ;;
*)
  echo "Internal error: Unknown command: $command"
  exit 1
esac
