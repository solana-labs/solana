#!/usr/bin/env bash
set -e

# TODO: Make sure a dB named $TESTNET_TAG exists in the influxDB host, or can be created
[[ -n $TESTNET_TAG ]] || TESTNET_TAG=testnet-automation
[[ -n $INFLUX_HOST ]] || INFLUX_HOST=https://metrics.solana.com:8086

# TODO: Remove all default values, force explicitness in the testcase definition
[[ -n $TEST_DURATION_SECONDS ]] || TEST_DURATION_SECONDS=300
[[ -n $RAMP_UP_TIME ]] || RAMP_UP_TIME=0
[[ -n $NUMBER_OF_VALIDATOR_NODES ]] || NUMBER_OF_VALIDATOR_NODES=2
[[ -n $NUMBER_OF_CLIENT_NODES ]] || NUMBER_OF_CLIENT_NODES=1

function collect_logs {
  echo --- collect logs from remote nodes
  rm -rf net/log
  net/net.sh logs
  for logfile in net/log/* ; do
    (
      new_log=net/log/"$TESTNET_TAG"_"$NUMBER_OF_VALIDATOR_NODES"-nodes_"$(basename "$logfile")"
      cp "$logfile" "$new_log"
      upload-ci-artifact "$new_log"
    )
  done
}

function cleanup_testnet {
  FINISH_UNIX_MSECS="$(($(date +%s%N)/1000000))"
  if [[ -n $UPLOAD_RESULTS_TO_SLACK ]] ; then
    upload_results_to_slack
  fi

  (
    set +e
    collect_logs
  )

  (
    set +e
    echo --- Stop Network Software
    net/net.sh stop
  )

  case $CLOUD_PROVIDER in
  gce)
  (
    cat <<EOF
- wait: ~
  continue_on_failure: true

- command: "net/gce.sh delete -p ${TESTNET_TAG}"
  label: "Delete Testnet"
  agents:
    - "queue=testnet-deploy"
EOF
  ) | buildkite-agent pipeline upload
  ;;
  colo)
    (
    cat <<EOF
- wait: ~
  continue_on_failure: true

- command: "net/colo.sh delete -p ${TESTNET_TAG}"
  label: "Delete Testnet"
  agents:
    - "queue=colo-deploy"
EOF
  ) | buildkite-agent pipeline upload
  ;;
  *)
    echo "Error: Unsupported cloud provider: $CLOUD_PROVIDER"
    ;;
  esac
}
trap cleanup_testnet EXIT

launchTestnet() {
  set -x

  # shellcheck disable=SC2068
  echo --- create "$NUMBER_OF_VALIDATOR_NODES" nodes

  case $CLOUD_PROVIDER in
    gce)
    # shellcheck disable=SC2068
      net/gce.sh create \
        -d pd-ssd \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" \
        "$maybeMachineType" "$VALIDATOR_NODE_MACHINE_TYPE" \
        -p "$TESTNET_TAG" ${TESTNET_CLOUD_ZONES[@]/#/"-z "} ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    colo)
    # shellcheck disable=SC2068
      net/colo.sh create \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" -g \
        -p "$TESTNET_TAG" ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    *)
      echo "Error: Unsupported cloud provider: $CLOUD_PROVIDER"
      ;;
    esac

  echo --- configure database
  net/init-metrics.sh -e

  echo --- start "$NUMBER_OF_VALIDATOR_NODES" node test
  if [[ -n $CHANNEL ]]; then
    net/net.sh restart -t "$CHANNEL" "$maybeClientOptions" "$CLIENT_OPTIONS"
  else
    net/net.sh restart -T solana-release*.tar.bz2 "$maybeClientOptions" "$CLIENT_OPTIONS"
  fi

  echo --- wait "$RAMP_UP_TIME" seconds for network throughput to stabilize
  sleep "$RAMP_UP_TIME"

  echo --- wait "$TEST_DURATION_SECONDS" seconds to complete test
  sleep "$TEST_DURATION_SECONDS"

  echo --- collect statistics about run
  declare q_mean_tps='
    SELECT ROUND(MEAN("median_sum")) as "mean_tps" FROM (
      SELECT MEDIAN(sum_count) AS "median_sum" FROM (
        SELECT SUM("count") AS "sum_count"
          FROM "'$TESTNET_TAG'"."autogen"."bank-process_transactions"
          WHERE time > now() - '"$TEST_DURATION_SECONDS"'s AND count > 0
          GROUP BY time(1s), host_id)
      GROUP BY time(1s)
    )'

  declare q_max_tps='
    SELECT MAX("median_sum") as "max_tps" FROM (
      SELECT MEDIAN(sum_count) AS "median_sum" FROM (
        SELECT SUM("count") AS "sum_count"
          FROM "'$TESTNET_TAG'"."autogen"."bank-process_transactions"
          WHERE time > now() - '"$TEST_DURATION_SECONDS"'s AND count > 0
          GROUP BY time(1s), host_id)
      GROUP BY time(1s)
    )'

  declare q_mean_confirmation='
    SELECT round(mean("duration_ms")) as "mean_confirmation_ms"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION_SECONDS"'s'

  declare q_max_confirmation='
    SELECT round(max("duration_ms")) as "max_confirmation_ms"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION_SECONDS"'s'

  declare q_99th_confirmation='
    SELECT round(percentile("duration_ms", 99)) as "99th_percentile_confirmation_ms"
      FROM "'$TESTNET_TAG'"."autogen"."validator-confirmation"
      WHERE time > now() - '"$TEST_DURATION_SECONDS"'s'

  curl -G "${INFLUX_HOST}/query?u=ro&p=topsecret" \
    --data-urlencode "db=${TESTNET_TAG}" \
    --data-urlencode "q=$q_mean_tps;$q_max_tps;$q_mean_confirmation;$q_max_confirmation;$q_99th_confirmation" |
    python system-test/testnet-performance/testnet-automation-json-parser.py >>"$RESULT_FILE"

  RESULT_DETAILS=$(<"$RESULT_FILE")
  upload-ci-artifact "$RESULT_FILE"
}

cd "$(dirname "$0")/../.."

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  if [[ -z $SOLANA_METRICS_PARTIAL_CONFIG ]]; then
    echo SOLANA_METRICS_PARTIAL_CONFIG not defined
    exit 1
  fi
  export SOLANA_METRICS_CONFIG="db=$TESTNET_TAG,host=$INFLUX_HOST,$SOLANA_METRICS_PARTIAL_CONFIG"
fi
echo "SOLANA_METRICS_CONFIG: $SOLANA_METRICS_CONFIG"

if [[ -z $CHANNEL ]]; then
  echo --- downloading tar from build artifacts
  buildkite-agent artifact download "solana-release*.tar.bz2" .
fi

# shellcheck disable=SC1091
source ci/upload-ci-artifact.sh
source system-test/testnet-performance/upload_results_to_slack.sh

maybeClientOptions=${CLIENT_OPTIONS:+"-c"}
maybeMachineType=${VALIDATOR_NODE_MACHINE_TYPE:+"-G"}

IFS=, read -r -a TESTNET_CLOUD_ZONES <<<"${TESTNET_ZONES}"

RESULT_FILE="$TESTNET_TAG"_SUMMARY_STATS_"$NUMBER_OF_VALIDATOR_NODES".log
rm -f $RESULT_FILE
RESULT_DETAILS="Test failed to finish"

TEST_PARAMS_TO_DISPLAY=(CLOUD_PROVIDER \
                        NUMBER_OF_VALIDATOR_NODES \
                        VALIDATOR_NODE_MACHINE_TYPE \
                        NUMBER_OF_CLIENT_NODES \
                        CLIENT_OPTIONS \
                        TESTNET_ZONES \
                        TEST_DURATION_SECONDS \
                        ADDITIONAL_FLAGS)

TEST_CONFIGURATION=
for i in "${TEST_PARAMS_TO_DISPLAY[@]}" ; do
  if [[ -n ${!i} ]] ; then
    TEST_CONFIGURATION+="${i} = ${!i} | "
  fi
done

START_UNIX_MSECS="$(($(date +%s%N)/1000000))"

launchTestnet
