#!/usr/bin/env bash
set -e

[[ -n $TESTNET_TAG ]] || TESTNET_TAG=testnet-automation

function cleanup_testnet {
  echo --- delete testnet
  net/gce.sh delete -p $TESTNET_TAG
}
trap cleanup_testnet EXIT

cd "$(dirname "$0")/../.."

if [[ -z $USE_PREBUILT_CHANNEL_TARBALL ]]; then
  echo --- downloading tar from build artifacts
  buildkite-agent artifact download "solana-release*.tar.bz2" .
fi

# shellcheck disable=SC1091
source ci/upload-ci-artifact.sh

[[ -n $INFLUX_HOST ]] || INFLUX_HOST=https://metrics.solana.com:8086
[[ -n $TEST_DURATION ]] || TEST_DURATION=300
[[ -n $RAMP_UP_TIME ]] || RAMP_UP_TIME=60
[[ -n $NUMBER_OF_VALIDATOR_NODES ]] || NUMBER_OF_VALIDATOR_NODES="10 25 50 100"
[[ -n $VALIDATOR_NODE_MACHINE_TYPE ]] ||
  VALIDATOR_NODE_MACHINE_TYPE="--machine-type n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100"
[[ -n $NUMBER_OF_CLIENT_NODES ]] || NUMBER_OF_CLIENT_NODES=2
[[ -n $CLIENT_OPTIONS ]] || CLIENT_OPTIONS=
[[ -n $TESTNET_ZONES ]] || TESTNET_ZONES="us-west1-b"
[[ -n $CHANNEL ]] || CHANNEL=beta
[[ -n $ADDITIONAL_FLAGS ]] || ADDITIONAL_FLAGS=""

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  if [[ -z $SOLANA_METRICS_PARTIAL_CONFIG ]]; then
    echo SOLANA_METRICS_PARTIAL_CONFIG not defined
    exit 1
  fi
  export SOLANA_METRICS_CONFIG="db=$TESTNET_TAG,host=$INFLUX_HOST,$SOLANA_METRICS_PARTIAL_CONFIG"
fi
echo "SOLANA_METRICS_CONFIG: $SOLANA_METRICS_CONFIG"

maybeClientOptions=
if [[ -n $CLIENT_OPTIONS ]] ; then
  maybeClientOptions="-c"
fi

TESTNET_CLOUD_ZONES=(); while read -r -d, ; do TESTNET_CLOUD_ZONES+=( "$REPLY" ); done <<< "${TESTNET_ZONES},"

launchTestnet() {
  declare nodeCount=$1
  echo --- setup "$nodeCount" node test

  set -x

  # shellcheck disable=SC2068
  echo --- create "$nodeCount" nodes
  net/gce.sh create \
    -d pd-ssd \
    -n "$nodeCount" -c "$NUMBER_OF_CLIENT_NODES" \
    -G "$VALIDATOR_NODE_MACHINE_TYPE" \
    -p "$TESTNET_TAG" ${TESTNET_CLOUD_ZONES[@]/#/-z } "$ADDITIONAL_FLAGS"

  echo --- configure database
  net/init-metrics.sh -e

  echo --- start "$nodeCount" node test
  if [[ -n $USE_PREBUILT_CHANNEL_TARBALL ]]; then
    net/net.sh start -t "$CHANNEL" "$maybeClientOptions" "$CLIENT_OPTIONS"
  else
    net/net.sh start -T solana-release*.tar.bz2 "$maybeClientOptions" "$CLIENT_OPTIONS"
  fi

  echo --- wait "$RAMP_UP_TIME" seconds for network throughput to stabilize
  sleep "$RAMP_UP_TIME"

  echo --- wait "$TEST_DURATION" seconds to complete test
  sleep "$TEST_DURATION"

  echo --- collect statistics about run
  declare q_mean_tps='
    SELECT round(mean("sum_count")) AS "mean_tps" FROM (
      SELECT sum("count") AS "sum_count"
        FROM "'$TESTNET_TAG'"."autogen"."banking_stage-record_transactions"
        WHERE time > now() - '"$TEST_DURATION"'s GROUP BY time(1s)
    )'

  declare q_max_tps='
    SELECT round(max("sum_count")) AS "max_tps" FROM (
      SELECT sum("count") AS "sum_count"
        FROM "'$TESTNET_TAG'"."autogen"."banking_stage-record_transactions"
        WHERE time > now() - '"$TEST_DURATION"'s GROUP BY time(1s)
    )'

  declare q_mean_confirmation='
    SELECT round(mean("duration_ms")) as "mean_confirmation"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION"'s'

  declare q_max_confirmation='
    SELECT round(max("duration_ms")) as "max_confirmation"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION"'s'

  declare q_99th_confirmation='
    SELECT round(percentile("duration_ms", 99)) as "99th_confirmation"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION"'s'

  RESULTS_FILE="$TESTNET_TAG"_SUMMARY_STATS_"$nodeCount".log
  curl -G "${INFLUX_HOST}/query?u=ro&p=topsecret" \
    --data-urlencode "db=${TESTNET_TAG}" \
    --data-urlencode "q=$q_mean_tps;$q_max_tps;$q_mean_confirmation;$q_max_confirmation;$q_99th_confirmation" |
    python tests/testnet-performance/testnet-automation-json-parser.py >>"$RESULTS_FILE"

  upload-ci-artifact "$RESULTS_FILE"

  echo --- collect logs from remote nodes
  rm -rf net/log
  net/net.sh logs
  for logfile in net/log/* ; do
    new_log=net/log/"$TESTNET_TAG"_"$nodeCount"-nodes_"$(basename "$logfile")"
    cp "$logfile" "$new_log"
    upload-ci-artifact "$new_log"
  done
}

# This is needed, because buildkite doesn't let us define an array of numbers.
# The array is defined as a space separated string of numbers
# shellcheck disable=SC2206
nodes_count_array=($NUMBER_OF_VALIDATOR_NODES)

for n in "${nodes_count_array[@]}"; do
  launchTestnet "$n"
done
