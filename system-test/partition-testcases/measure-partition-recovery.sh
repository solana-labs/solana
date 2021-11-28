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

if [[ -z $PARTITION_INCREMENT ]]; then
  PARTITION_INCREMENT=60
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
target=$mean_confirmation_ms

while true; do
  execution_step "Applying partition config $NETEM_CONFIG_FILE for $PARTITION_DURATION seconds"
  echo "Partitioning for $PARTITION_DURATION seconds" >> "$RESULT_FILE"
  "${REPO_ROOT}"/net/net.sh netem --config-file "$NETEM_CONFIG_FILE" -n $num_online_nodes
  sleep "$PARTITION_DURATION"

  execution_step "Resolving partition"
  "${REPO_ROOT}"/net/net.sh netem --config-file "$NETEM_CONFIG_FILE" --netem-cmd cleanup -n $num_online_nodes

  get_validator_confirmation_time 10
  SECONDS=0

  # This happens when we haven't confirmed anything recently so the query returns an empty string
  while [[ -z $mean_confirmation_ms ]]; do
    sleep 5
    get_validator_confirmation_time 10
    if [[ $SECONDS -gt $PARTITION_DURATION ]]; then
      echo "  No confirmations seen after $SECONDS seconds" >> "$RESULT_FILE"
      exit 0
    fi
  done
  echo "  Validator confirmation is $mean_confirmation_ms ms $SECONDS seconds after resolving the partition" >> "$RESULT_FILE"

  last=""
  while [[ -z $mean_confirmation_ms || $mean_confirmation_ms -gt $target ]]; do
    sleep 5

    if [[ -n $mean_confirmation_ms && -n $last && $mean_confirmation_ms -gt $(echo "$last * 1.2" | bc) || $SECONDS -gt $PARTITION_DURATION ]]; then
      echo "  Unable to make progress after $SECONDS seconds. Last confirmation time was $mean_confirmation_ms ms" >> "$RESULT_FILE"
      exit 0
    fi
    last=$mean_confirmation_ms
    get_validator_confirmation_time 10
  done

  echo "  Recovered in $SECONDS seconds: validator confirmation to fall to $mean_confirmation_ms ms" >> "$RESULT_FILE"

  PARTITION_DURATION=$(( PARTITION_DURATION + PARTITION_INCREMENT ))
done
