#!/bin/bash -e

#
# Save target/ for the next CI build on this machine
#
if [[ -z $CARGO_TARGET_CACHE ]]; then
  echo "+++ CARGO_TARGET_CACHE not defined" # pre-command should have defined it
else
  (
    set -x
    mkdir -p "$CARGO_TARGET_CACHE"
    set -x
    rsync -a --delete --link-dest="$PWD" target "$CARGO_TARGET_CACHE"
    du -hs "$CARGO_TARGET_CACHE"
    read -r cacheSizeInGB _ < <(du -s --block-size=1800000000 "$CARGO_TARGET_CACHE")
    echo "--- ${cacheSizeInGB}GB: $CARGO_TARGET_CACHE"
  )
fi

#
# Add job_stats data point
#
if [[ -z $CI_BUILD_START ]]; then
  echo Error: CI_BUILD_START empty
else
  CI_BUILD_DURATION=$(( $(date +%s) - CI_BUILD_START + 1 ))

  CI_LABEL=${BUILDKITE_LABEL:-build label missing}

  PR=false
  if [[ $BUILDKITE_BRANCH =~ pull/* ]]; then
    PR=true
  fi

  SUCCESS=true
  if [[ $BUILDKITE_COMMAND_EXIT_STATUS != 0 ]]; then
    SUCCESS=false
  fi

  point_tags="pipeline=$BUILDKITE_PIPELINE_SLUG,job=$CI_LABEL,pr=$PR,success=$SUCCESS"
  point_tags="${point_tags// /\\ }"  # Escape spaces

  point_fields="duration=$CI_BUILD_DURATION"
  point_fields="${point_fields// /\\ }"  # Escape spaces

  point="job_stats,$point_tags $point_fields"

  scripts/metrics-write-datapoint.sh "$point" || true
fi
