upload_results_to_slack() {
  echo --- Uploading results to Slack Performance Results App

  if [[ -z $SLACK_WEBHOOK_URL ]] ; then
    echo "SLACK_WEBHOOOK_URL undefined"
    exit 1
  fi

  [[ -n $BUILDKITE_MESSAGE ]] || BUILDKITE_MESSAGE="Message not defined"

  if [[ -n $BUILDKITE_COMMIT ]] ; then
    COMMIT_BUTTON_TEXT="$(echo "$BUILDKITE_COMMIT" | head -c 8)"
    COMMIT_URL="https://github.com/solana-labs/solana/commit/${BUILDKITE_COMMIT}"
  else
    COMMIT_BUTTON_TEXT="Commit not defined"
    COMMIT_URL="https://github.com/solana-labs/solana/commits/master"
  fi

  if [[ -n $BUILDKITE_BUILD_URL ]] ; then
    BUILD_BUTTON_TEXT="Build Kite Job"
  else
    BUILD_BUTTON_TEXT="Build URL not defined"
    BUILDKITE_BUILD_URL="https://buildkite.com/solana-labs/"
  fi

  GRAFANA_URL="https://metrics.solana.com:3000/d/testnet-${CHANNEL:-edge}/testnet-monitor-${CHANNEL:-edge}?var-testnet=${TESTNET_TAG:-testnet-automation}&from=${START_UNIX_MSECS:-0}&to=${FINISH_UNIX_MSECS:-0}"

  [[ -n $RESULT_DETAILS ]] || RESULT_DETAILS="Undefined"
  [[ -n $TEST_CONFIGURATION ]] || TEST_CONFIGURATION="Undefined"

  payLoad="$(cat <<EOF
{
"blocks": [
 		{
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "*New Build: $BUILDKITE_MESSAGE*"
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
