#!/usr/bin/env bash

set -ex

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

RESULT_FILE="$1"

[[ -n $TESTNET_TAG ]] || TESTNET_TAG=${CLOUD_PROVIDER}-testnet-automation

if [[ -z $NETEM_CONFIG_FILE  ]]; then
  echo "Error: For this test NETEM_CONFIG_FILE must be specified"
  exit 1
fi

if [[ -z $PRE_PARTITION_DURATION ]]; then
  PRE_PARTITION_DURATION=60
fi

if [[ -z $PARTITION_DURATION ]]; then
  PARTITION_DURATION=300
fi

num_online_nodes=$(( NUMBER_OF_VALIDATOR_NODES + 1 ))
if [[ -n "$NUMBER_OF_OFFLINE_NODES" ]]; then
  num_online_nodes=$(( num_online_nodes - NUMBER_OF_OFFLINE_NODES ))
fi

execution_step "Measuring validator confirmation time for $PRE_PARTITION_DURATION seconds"
sleep "$PRE_PARTITION_DURATION"
get_validator_confirmation_time "$PRE_PARTITION_DURATION"
# shellcheck disable=SC2154
execution_step "Pre partition validator confirmation time is $mean_confirmation_ms ms"
echo "Pre partition validator confirmation time: $mean_confirmation_ms ms" >> "$RESULT_FILE"

execution_step "Applying partition config $NETEM_CONFIG_FILE for $PARTITION_DURATION seconds"
"${REPO_ROOT}"/net/net.sh netem --config-file "$NETEM_CONFIG_FILE" -n $num_online_nodes
sleep "$PARTITION_DURATION"

execution_step "Resolving partition"
"${REPO_ROOT}"/net/net.sh netem --config-file "$NETEM_CONFIG_FILE" --netem-cmd cleanup -n $num_online_nodes

target=$mean_confirmation_ms
get_validator_confirmation_time 10
time=0
echo "Validator confirmation is $mean_confirmation_ms ms immediately after the partition" >> "$RESULT_FILE"

while [[ $mean_confirmation_ms == "expected" || $mean_confirmation_ms -gt $target ]]; do
  sleep 1
  time=$(( time + 1 ))
  get_validator_confirmation_time 10
done

echo "$time seconds after resolving the partition, validator confirmation time fell to $mean_confirmation_ms" >> "$RESULT_FILE"
