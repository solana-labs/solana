#!/usr/bin/env bash

# | source | this file
# shellcheck disable=SC1090
# shellcheck disable=SC1091
# shellcheck disable=SC2034

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
REPO_ROOT=${DIR}/..

source "${REPO_ROOT}"/ci/upload-ci-artifact.sh

function execution_step {
  # shellcheck disable=SC2124
  STEP="$@"
  echo --- "${STEP[@]}"
}

function collect_logs {
  execution_step "Collect logs from remote nodes"
  rm -rf "${REPO_ROOT}"/net/log
  "${REPO_ROOT}"/net/net.sh logs
  for logfile in "${REPO_ROOT}"/net/log/*; do
    (
      upload-ci-artifact "$logfile"
    )
  done
}

function analyze_packet_loss {
  (
    set -x
    # shellcheck disable=SC1091
    source "${REPO_ROOT}"/net/config/config
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
      "${REPO_ROOT}"/net/scp.sh ip_address_map.txt solana@"$ip":~/solana/
    done

    execution_step "Remotely post-process iftop logs"
    # shellcheck disable=SC2154
    for ip in "${validatorIpList[@]}"; do
      iftop_log=iftop-logs/$ip-iftop.log
      # shellcheck disable=SC2016
      "${REPO_ROOT}"/net/ssh.sh solana@"$ip" 'PATH=$PATH:~/.cargo/bin/ ~/solana/scripts/iftop-postprocess.sh ~/solana/iftop.log temp.log ~solana/solana/ip_address_map.txt' > "$iftop_log"
      upload-ci-artifact "$iftop_log"
    done

    execution_step "Analyzing Packet Loss"
    "${REPO_ROOT}"/solana-release/bin/solana-log-analyzer analyze -f ./iftop-logs/ | sort -k 2 -g
  )
}

function wait_for_bootstrap_validator_stake_drop {
  max_stake="$1"
  if [[ $max_stake -eq 100 ]]; then
    return
  fi

  source "${REPO_ROOT}"/net/common.sh
  loadConfigFile

  while true; do
  # shellcheck disable=SC2154
    bootstrap_validator_validator_info="$(ssh "${sshOptions[@]}" "${validatorIpList[0]}" '$HOME/.cargo/bin/solana --url http://127.0.0.1:8899 validators | grep "$($HOME/.cargo/bin/solana-keygen pubkey ~/solana/config/bootstrap-validator/identity.json)"')"
    bootstrap_validator_stake_percentage="$(echo "$bootstrap_validator_validator_info" | awk '{gsub(/[\(,\),\%]/,""); print $9}')"

    if [[ $(echo "$bootstrap_validator_stake_percentage < $max_stake" | bc) -ne 0 ]]; then
      echo "Bootstrap validator stake has fallen below $max_stake to $bootstrap_validator_stake_percentage"
      break
    fi
    echo "Max bootstrap validator stake: $max_stake.  Current stake: $bootstrap_validator_stake_percentage.  Sleeping 30s for stake to distribute."
    sleep 30
  done
}

function get_slot {
  source "${REPO_ROOT}"/net/common.sh
  loadConfigFile
  ssh "${sshOptions[@]}" "${validatorIpList[0]}" '$HOME/.cargo/bin/solana slot'
}

function get_bootstrap_validator_ip_address {
  source "${REPO_ROOT}"/net/common.sh
  loadConfigFile
  echo "${validatorIpList[0]}"
}

function collect_performance_statistics {
  execution_step "Collect performance statistics about run"
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

  declare q_max_tower_distance_observed='
    SELECT MAX("tower_distance") as "max_tower_distance" FROM (
      SELECT last("slot") - last("root") as "tower_distance"
        FROM "'$TESTNET_TAG'"."autogen"."tower-observed"
        WHERE time > now() - '"$TEST_DURATION_SECONDS"'s
        GROUP BY time(1s), host_id)'

  declare q_last_tower_distance_observed='
      SELECT MEAN("tower_distance") as "last_tower_distance" FROM (
            SELECT last("slot") - last("root") as "tower_distance"
              FROM "'$TESTNET_TAG'"."autogen"."tower-observed"
              GROUP BY host_id)'

  curl -G "${INFLUX_HOST}/query?u=ro&p=topsecret" \
    --data-urlencode "db=${TESTNET_TAG}" \
    --data-urlencode "q=$q_mean_tps;$q_max_tps;$q_mean_confirmation;$q_max_confirmation;$q_99th_confirmation;$q_max_tower_distance_observed;$q_last_tower_distance_observed" |
    python "${REPO_ROOT}"/system-test/testnet-automation-json-parser.py >>"$RESULT_FILE"
}

function upload_results_to_slack() {
  echo --- Uploading results to Slack Performance Results App

  if [[ -z $SLACK_WEBHOOK_URL ]] ; then
    echo "SLACK_WEBHOOOK_URL undefined"
    exit 1
  fi

  [[ -n $BUILDKITE_MESSAGE ]] || BUILDKITE_MESSAGE="Message not defined"

  COMMIT=$(git rev-parse HEAD)
  COMMIT_BUTTON_TEXT="$(echo "$COMMIT" | head -c 8)"
  COMMIT_URL="https://github.com/solana-labs/solana/commit/${COMMIT}"

  if [[ -n $BUILDKITE_BUILD_URL ]] ; then
    BUILD_BUTTON_TEXT="Build Kite Job"
  else
    BUILD_BUTTON_TEXT="Build URL not defined"
    BUILDKITE_BUILD_URL="https://buildkite.com/solana-labs/"
  fi

  GRAFANA_URL="https://metrics.solana.com:3000/d/monitor-${CHANNEL:-edge}/cluster-telemetry-${CHANNEL:-edge}?var-testnet=${TESTNET_TAG:-testnet-automation}&from=${TESTNET_START_UNIX_MSECS:-0}&to=${TESTNET_FINISH_UNIX_MSECS:-0}"

  [[ -n $RESULT_DETAILS ]] || RESULT_DETAILS="Undefined"
  [[ -n $TEST_CONFIGURATION ]] || TEST_CONFIGURATION="Undefined"

  payLoad="$(cat <<EOF
{
"blocks": [
        {
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "*$BUILDKITE_MESSAGE*"
			}
		},
    {
			"type": "actions",
			"elements": [
				{
					"type": "button",
					"text": {
						"type": "plain_text",
						"text": "$COMMIT_BUTTON_TEXT",
						"emoji": true
					},
					"url": "$COMMIT_URL"
				},
        {
					"type": "button",
					"text": {
						"type": "plain_text",
						"text": "$BUILD_BUTTON_TEXT",
						"emoji": true
					},
					"url": "$BUILDKITE_BUILD_URL"
				},
        {
					"type": "button",
					"text": {
						"type": "plain_text",
						"text": "Grafana",
						"emoji": true
					},
					"url": "$GRAFANA_URL"
				}
			]
		},
		{
			"type": "divider"
    },
    {
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "Test Configuration: \n\`\`\`$TEST_CONFIGURATION\`\`\`"
			}
		},
		{
			"type": "divider"
		},
    {
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "Result Details: \n\`\`\`$RESULT_DETAILS\`\`\`"
			}
		}
	]
}
EOF
)"

  curl -X POST \
  -H 'Content-type: application/json' \
  --data "$payLoad" \
  "$SLACK_WEBHOOK_URL"
}
