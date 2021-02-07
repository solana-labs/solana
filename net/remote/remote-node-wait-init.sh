#!/usr/bin/env bash
set -e
set -x

initCompleteFile=init-complete-node.log
waitTime=${1:=600}

waitForNodeToInit() {
  declare hostname
  hostname=$(hostname)
  echo "--- waiting for $hostname to boot up"
  declare startTime=$SECONDS
  while [[ ! -r $initCompleteFile ]]; do
    declare timeWaited=$((SECONDS - startTime))
    if [[ $timeWaited -ge $waitTime ]]; then
      echo "^^^ +++"
      echo "Error: $initCompleteFile not found in $timeWaited seconds"
      exit 1
    fi
    echo "Waiting for $initCompleteFile ($timeWaited) on $hostname..."
    sleep 5
  done
  echo "$hostname booted up"
}

cd ~/solana
waitForNodeToInit
