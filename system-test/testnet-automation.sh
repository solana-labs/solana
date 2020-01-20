#!/usr/bin/env bash
set -e

function execution_step {
  # shellcheck disable=SC2124
  STEP="$@"
  echo --- "${STEP[@]}"
}

function collect_logs {
  execution_step "Collect logs from remote nodes"
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

function analyze_packet_loss {
  (
    set -x
    # shellcheck disable=SC1091
    source net/config/config
    mkdir -p iftop-logs
    execution_step "Map private -> public IP addresses in iftop logs"
    # shellcheck disable=SC2154
    for i in "${!validatorIpList[@]}"; do
      # shellcheck disable=SC2154
      # shellcheck disable=SC2086
      # shellcheck disable=SC2027
      echo "{\"private\": \""${validatorIpListPrivate[$i]}""\", \"public\": \""${validatorIpList[$i]}""\"},"
    done > ip_address_map.txt

    for ip in "${validatorIpList[@]}"; do
      net/scp.sh ip_address_map.txt solana@"$ip":~/solana/
    done

    execution_step "Remotely post-process iftop logs"
    # shellcheck disable=SC2154
    for ip in "${validatorIpList[@]}"; do
      iftop_log=iftop-logs/$ip-iftop.log
      # shellcheck disable=SC2016
      net/ssh.sh solana@"$ip" 'PATH=$PATH:~/.cargo/bin/ ~/solana/scripts/iftop-postprocess.sh ~/solana/iftop.log temp.log ~solana/solana/ip_address_map.txt' > "$iftop_log"
      upload-ci-artifact "$iftop_log"
    done

    execution_step "Analyzing Packet Loss"
    solana-release/bin/solana-log-analyzer analyze -f ./iftop-logs/ | sort -k 2 -g
  )
}

function wait_for_bootstrap_leader_stake_drop {
  max_stake="$1"
  source net/common.sh
  loadConfigFile

  while true; do
    bootstrap_leader_validator_info="$(ssh "${sshOptions[@]}" "${validatorIpList[0]}" '$HOME/.cargo/bin/solana show-validators | grep "$($HOME/.cargo/bin/solana-keygen pubkey ~/solana/config/bootstrap-leader/identity-keypair.json)"')"
    bootstrap_leader_stake_percentage="$(echo "$bootstrap_leader_validator_info" | awk '{gsub(/[\(,\),\%]/,""); print $9}')"

    if [[ $(echo "$bootstrap_leader_stake_percentage < $max_stake" | bc) -ne 0 ]]; then
      echo "Bootstrap leader stake has fallen below $max_stake to $bootstrap_leader_stake_percentage"
      break
    fi
    echo "Max bootstrap leader stake: $max_stake.  Current stake: $bootstrap_leader_stake_percentage.  Sleeping 30s for stake to distribute."
    sleep 30
  done
}

function cleanup_testnet {
  RC=$?
  if [[ $RC != 0 ]] ; then
    RESULT_DETAILS="
Test failed during step:
${STEP}

Failure occured when running the following command:
$(eval echo "$@")"
  fi

  FINISH_UNIX_MSECS="$(($(date +%s%N)/1000000))"
  if [[ "$UPLOAD_RESULTS_TO_SLACK" = "true" ]] ; then
    upload_results_to_slack
  fi

  (
    set +e
    execution_step "Collecting Logfiles from Nodes"
    collect_logs
  )

  (
    set +e
    execution_step "Stop Network Software"
    net/net.sh stop
  )

  (
    set +e
    analyze_packet_loss
  )

  execution_step "Deleting Testnet"
  net/"${CLOUD_PROVIDER}".sh delete -p "${TESTNET_TAG}"

}
trap 'cleanup_testnet $BASH_COMMAND' EXIT

function launchTestnet() {
  set -x

  # shellcheck disable=SC2068
  execution_step "Create ${NUMBER_OF_VALIDATOR_NODES} ${CLOUD_PROVIDER} nodes"

  case $CLOUD_PROVIDER in
    gce)
      if [[ -z $VALIDATOR_NODE_MACHINE_TYPE ]] ; then
        echo VALIDATOR_NODE_MACHINE_TYPE not defined
        exit 1
      fi
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
      net/gce.sh create \
        -d pd-ssd \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" \
        $maybeCustomMachineType "$VALIDATOR_NODE_MACHINE_TYPE" $maybeEnableGpu \
        -p "$TESTNET_TAG" $maybeCreateAllowBootFailures $maybePublicIpAddresses \
        ${TESTNET_CLOUD_ZONES[@]/#/"-z "} \
        ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    ec2)
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
      net/ec2.sh create \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" \
        $maybeCustomMachineType "$VALIDATOR_NODE_MACHINE_TYPE" $maybeEnableGpu \
        -p "$TESTNET_TAG" $maybeCreateAllowBootFailures $maybePublicIpAddresses \
        ${TESTNET_CLOUD_ZONES[@]/#/"-z "} \
        ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    azure)
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
      net/azure.sh create \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" \
        $maybeCustomMachineType "$VALIDATOR_NODE_MACHINE_TYPE" $maybeEnableGpu \
        -p "$TESTNET_TAG" $maybeCreateAllowBootFailures $maybePublicIpAddresses \
        ${TESTNET_CLOUD_ZONES[@]/#/"-z "} \
        ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    colo)
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
      net/colo.sh create \
        -n "$NUMBER_OF_VALIDATOR_NODES" -c "$NUMBER_OF_CLIENT_NODES" $maybeEnableGpu \
        -p "$TESTNET_TAG" $maybePublicIpAddresses \
        ${ADDITIONAL_FLAGS[@]/#/" "}
      ;;
    *)
      echo "Error: Unsupported cloud provider: $CLOUD_PROVIDER"
      ;;
    esac

  execution_step "Configure database"
  net/init-metrics.sh -e

  execution_step "Fetch reusable testnet keypairs"
  if [[ ! -d net/keypairs ]] ; then
    git clone git@github.com:solana-labs/testnet-keypairs.git net/keypairs
    # If we have provider-specific keys (CoLo*, GCE*, etc) use them instead of generic val*
    if [[ -d net/keypairs/"${CLOUD_PROVIDER}" ]] ; then
      cp net/keypairs/"${CLOUD_PROVIDER}"/* net/keypairs/
    fi
  fi

  if [[ "$CLOUD_PROVIDER" = "colo" ]] ; then
    execution_step "Stopping Colo nodes before we start"
    net/net.sh stop
  fi

  execution_step "Start ${NUMBER_OF_VALIDATOR_NODES} node test"
  if [[ -n $CHANNEL ]]; then
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
    net/net.sh start -t "$CHANNEL" \
      "$maybeClientOptions" "$CLIENT_OPTIONS" $maybeStartAllowBootFailures \
      --gpu-mode $startGpuMode --client-delay-start $CLIENT_DELAY_START
  else
    # shellcheck disable=SC2068
    # shellcheck disable=SC2086
    net/net.sh start -T solana-release*.tar.bz2 \
      "$maybeClientOptions" "$CLIENT_OPTIONS" $maybeStartAllowBootFailures \
      --gpu-mode $startGpuMode --client-delay-start $CLIENT_DELAY_START
  fi

  execution_step "Waiting for bootstrap leader's stake percentage to fall below $BOOTSTRAP_LEADER_MAX_STAKE_THRESHOLD %"
  wait_for_bootstrap_leader_stake_drop "$BOOTSTRAP_LEADER_MAX_STAKE_THRESHOLD"

  execution_step "Wait ${TEST_DURATION_SECONDS} seconds to complete test"
  sleep "$TEST_DURATION_SECONDS"

  execution_step "Collect statistics about run"
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
    python system-test/testnet-automation-json-parser.py >>"$RESULT_FILE"

  execution_step "Writing test results to ${RESULT_FILE}"
  RESULT_DETAILS=$(<"$RESULT_FILE")
  upload-ci-artifact "$RESULT_FILE"
}

RESULT_DETAILS=
STEP=
execution_step "Initialize Environment"

cd "$(dirname "$0")/.."

[[ -n $TESTNET_TAG ]] || TESTNET_TAG=testnet-automation
[[ -n $INFLUX_HOST ]] || INFLUX_HOST=https://metrics.solana.com:8086
[[ -n $BOOTSTRAP_LEADER_MAX_STAKE_THRESHOLD ]] || BOOTSTRAP_LEADER_MAX_STAKE_THRESHOLD=66

if [[ -z $TEST_DURATION_SECONDS ]] ; then
  echo TEST_DURATION_SECONDS not defined
  exit 1
fi

if [[ -z $NUMBER_OF_VALIDATOR_NODES ]] ; then
  echo NUMBER_OF_VALIDATOR_NODES not defined
  exit 1
fi

startGpuMode="off"
if [[ -z $ENABLE_GPU ]] ; then
  ENABLE_GPU=false
fi
if [[ "$ENABLE_GPU" = "true" ]] ; then
  maybeEnableGpu="--enable-gpu"
  startGpuMode="on"
fi

if [[ -z $NUMBER_OF_CLIENT_NODES ]] ; then
  echo NUMBER_OF_CLIENT_NODES not defined
  exit 1
fi

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  if [[ -z $SOLANA_METRICS_PARTIAL_CONFIG ]]; then
    echo SOLANA_METRICS_PARTIAL_CONFIG not defined
    exit 1
  fi
  export SOLANA_METRICS_CONFIG="db=$TESTNET_TAG,host=$INFLUX_HOST,$SOLANA_METRICS_PARTIAL_CONFIG"
fi
echo "SOLANA_METRICS_CONFIG: $SOLANA_METRICS_CONFIG"

if [[ -z $ALLOW_BOOT_FAILURES ]] ; then
  ALLOW_BOOT_FAILURES=false
fi
if [[ "$ALLOW_BOOT_FAILURES" = "true" ]] ; then
  maybeCreateAllowBootFailures="--allow-boot-failures"
  maybeStartAllowBootFailures="-F"
fi

if [[ -z $USE_PUBLIC_IP_ADDRESSES ]] ; then
  USE_PUBLIC_IP_ADDRESSES=false
fi
if [[ "$USE_PUBLIC_IP_ADDRESSES" = "true" ]] ; then
  maybePublicIpAddresses="-P"
fi

: "${CLIENT_DELAY_START:=0}"

if [[ -z $CHANNEL ]]; then
  execution_step "Downloading tar from build artifacts"
  buildkite-agent artifact download "solana-release*.tar.bz2" .
fi

# shellcheck disable=SC1091
source ci/upload-ci-artifact.sh
source system-test/upload_results_to_slack.sh

maybeClientOptions=${CLIENT_OPTIONS:+"-c"}
maybeCustomMachineType=${VALIDATOR_NODE_MACHINE_TYPE:+"--custom-machine-type"}

IFS=, read -r -a TESTNET_CLOUD_ZONES <<<"${TESTNET_ZONES}"

RESULT_FILE="$TESTNET_TAG"_SUMMARY_STATS_"$NUMBER_OF_VALIDATOR_NODES".log
rm -f "$RESULT_FILE"

TEST_PARAMS_TO_DISPLAY=(CLOUD_PROVIDER \
                        NUMBER_OF_VALIDATOR_NODES \
                        ENABLE_GPU \
                        VALIDATOR_NODE_MACHINE_TYPE \
                        NUMBER_OF_CLIENT_NODES \
                        CLIENT_OPTIONS \
                        CLIENT_DELAY_START \
                        TESTNET_ZONES \
                        TEST_DURATION_SECONDS \
                        USE_PUBLIC_IP_ADDRESSES \
                        ALLOW_BOOT_FAILURES \
                        ADDITIONAL_FLAGS)

TEST_CONFIGURATION=
for i in "${TEST_PARAMS_TO_DISPLAY[@]}" ; do
  if [[ -n ${!i} ]] ; then
    TEST_CONFIGURATION+="${i} = ${!i} | "
  fi
done

START_UNIX_MSECS="$(($(date +%s%N)/1000000))"

launchTestnet
