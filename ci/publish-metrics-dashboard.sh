#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

if [[ -z $BUILDKITE ]]; then
  echo BUILDKITE not defined
  exit 1
fi

if [[ -z $PUBLISH_CHANNEL ]]; then
  PUBLISH_CHANNEL=$(buildkite-agent meta-data get "channel" --default "")
fi

if [[ -z $PUBLISH_CHANNEL ]]; then
  (
    cat <<EOF
steps:
  - block: "Select Dashboard"
    fields:
      - select: "Channel"
        key: "channel"
        options:
          - label: "stable"
            value: "stable"
          - label: "edge"
            value: "edge"
          - label: "beta"
            value: "beta"
  - command: "ci/$(basename "$0")"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi


ci/channel-info.sh
eval "$(ci/channel-info.sh)"

case $PUBLISH_CHANNEL in
edge)
  CHANNEL_BRANCH=$EDGE_CHANNEL
  ;;
beta)
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
stable)
  # Set to whatever branch 'testnet' is on.
  # TODO: Revert to $STABLE_CHANNEL for TdS
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
*)
  echo "Error: Invalid PUBLISH_CHANNEL=$PUBLISH_CHANNEL"
  exit 1
  ;;
esac

if [[ $CI_BRANCH != "$CHANNEL_BRANCH" ]]; then
  (
    cat <<EOF
steps:
  - trigger: "$BUILDKITE_PIPELINE_SLUG"
    async: true
    build:
      message: "$BUILDKITE_MESSAGE"
      branch: "$CHANNEL_BRANCH"
      env:
        PUBLISH_CHANNEL: "$PUBLISH_CHANNEL"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi

set -x
exec metrics/publish-metrics-dashboard.sh "$PUBLISH_CHANNEL"
