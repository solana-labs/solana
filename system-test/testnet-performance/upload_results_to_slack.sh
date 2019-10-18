upload_results_to_slack() {
  echo --- Uploading results to Slack Performance Results App

  if [[ -z $SLACK_WEBHOOK_URL ]] ; then
    echo "SLACK_WEBHOOOK_URL undefined"
    exit 1
  fi

  [[ -n $BUILDKITE_MESSAGE ]] || BUILDKITE_MESSAGE="Message not defined"
  [[ -n $BUILDKITE_COMMIT ]] || BUILDKITE_COMMIT="Commit not defined"
  [[ -n $BUILDKITE_BUILD_URL ]] || BUILDKITE_BUILD_URL="Build URL not defined"
  [[ -n $GRAFANA_URL ]] || GRAFANA_URL="Undefined"
  [[ -n $RESULT_DETAILS ]] || RESULT_DETAILS="Undefined"
  [[ -n $TEST_CONFIGURATION ]] || TEST_CONFIGURATION="Undefined"

  payLoad="$(cat <<EOF
{
"blocks": [
 		{
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text":

"*New Build Started at: $START_TIME*
Buildkite Message: $BUILDKITE_MESSAGE
Commit SHA: $BUILDKITE_COMMIT
Link to Build: $BUILDKITE_BUILD_URL
Link to Grafana: $GRAFANA_URL
"
			},
		},
		{
			"type": "divider"
    },
    {
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "Test Configuration: \n\`\`\`$TEST_CONFIGURATION\`\`\`"
			},
		},
		{
			"type": "divider"
		},
 		{
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text": "Result Details: \n\`\`\`$RESULT_DETAILS\`\`\`"
			},
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
