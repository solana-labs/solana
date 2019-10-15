#!/usr/bin/env bash

upload_results() {

RESULT_DETAILS=$(<$RESULTS_FILE)

read -d '' payLoad << EOF
{
"blocks": [
 		{
			"type": "section",
			"text": {
				"type": "mrkdwn",
				"text":
"\*$BUILDKITE_MESSAGE\*
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

curl -X POST \
-H 'Content-type: application/json' \
--data "$payLoad" \
"https://hooks.slack.com/services/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"
}
