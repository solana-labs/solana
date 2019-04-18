#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

if [[ -z $USE_PREBUILT_CHANNEL_TARBALL ]]; then
  echo --- downloading tar from build artifacts
  buildkite-agent artifact download "solana*.tar.bz2" .
fi

# shellcheck disable=SC1091
source ci/upload-ci-artifact.sh

[[ -n $ITERATION_WAIT ]] || ITERATION_WAIT=300
[[ -n $NUMBER_OF_NODES ]] || NUMBER_OF_NODES="10 25 50 100"
[[ -n $LEADER_CPU_MACHINE_TYPE ]] ||
  LEADER_CPU_MACHINE_TYPE="--machine-type n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100"
[[ -n $CLIENT_COUNT ]] || CLIENT_COUNT=2
[[ -n $TESTNET_TAG ]] || TESTNET_TAG=testnet-automation
[[ -n $TESTNET_ZONES ]] || TESTNET_ZONES="us-west1-b"
[[ -n $CHANNEL ]] || CHANNEL=beta
[[ -n $ADDITIONAL_FLAGS ]] || ADDITIONAL_FLAGS=""

TESTNET_CLOUD_ZONES=(); while read -r -d, ; do TESTNET_CLOUD_ZONES+=( "$REPLY" ); done <<< "${TESTNET_ZONES},"

launchTestnet() {
  declare nodeCount=$1
  echo --- setup "$nodeCount" node test

  # shellcheck disable=SC2068
  net/gce.sh create \
    -d pd-ssd \
    -n "$nodeCount" -c "$CLIENT_COUNT" \
    -G "$LEADER_CPU_MACHINE_TYPE" \
    -p "$TESTNET_TAG" ${TESTNET_CLOUD_ZONES[@]/#/-z } "$ADDITIONAL_FLAGS"

  echo --- configure database
  net/init-metrics.sh -e

  echo --- start "$nodeCount" node test
  if [[ -n $USE_PREBUILT_CHANNEL_TARBALL ]]; then
    net/net.sh start -f "cuda" -o noValidatorSanity -t "$CHANNEL"
  else
    net/net.sh start -f "cuda" -o noValidatorSanity -T solana*.tar.bz2
  fi

  echo --- wait "$ITERATION_WAIT" seconds to complete test
  sleep "$ITERATION_WAIT"

  set -x

  declare q_mean_tps='
    SELECT round(mean("sum_count")) AS "mean_tps" FROM (
      SELECT sum("count") AS "sum_count"
        FROM "testnet-automation"."autogen"."counter-banking_stage-process_transactions"
        WHERE time > now() - 300s GROUP BY time(1s)
    )'

  declare q_max_tps='
    SELECT round(max("sum_count")) AS "max_tps" FROM (
      SELECT sum("count") AS "sum_count"
        FROM "testnet-automation"."autogen"."counter-banking_stage-process_transactions"
        WHERE time > now() - 300s GROUP BY time(1s)
    )'

  declare q_mean_confirmation='
    SELECT round(mean("duration_ms")) as "mean_confirmation"
      FROM "testnet-automation"."autogen"."validator-confirmation"
      WHERE time > now() - 300s'

  declare q_max_confirmation='
    SELECT round(max("duration_ms")) as "max_confirmation"
      FROM "testnet-automation"."autogen"."validator-confirmation"
      WHERE time > now() - 300s'

  declare q_99th_confirmation='
    SELECT round(percentile("duration_ms", 99)) as "99th_confirmation"
      FROM "testnet-automation"."autogen"."validator-confirmation"
      WHERE time > now() - 300s'

  curl -G "${INFLUX_HOST}/query?u=ro&p=topsecret" \
    --data-urlencode "db=testnet-automation" \
    --data-urlencode "q=$q_mean_tps;$q_max_tps;$q_mean_confirmation;$q_max_confirmation;$q_99th_confirmation" |
    python ci/testnet-automation-json-parser.py >>TPS"$nodeCount".log

  upload-ci-artifact TPS"$nodeCount".log
}

# This is needed, because buildkite doesn't let us define an array of numbers.
# The array is defined as a space separated string of numbers
# shellcheck disable=SC2206
nodes_count_array=($NUMBER_OF_NODES)

for n in "${nodes_count_array[@]}"; do
  launchTestnet "$n"
done
