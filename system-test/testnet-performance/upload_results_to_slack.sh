#!/usr/bin/env bash

upload_results() {
  payLoad="$(cat <<EOF
{
"blocks": [
 		{
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text":
"$BUILDKITE_MESSAGE
Commit SHA: $BUILDKITE_COMMIT
Link to Build: $BUILDKITE_BUILD_URL
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
				"text": "Result Details:
$RESULT_DETAILS"
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
