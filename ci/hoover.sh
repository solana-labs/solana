#!/usr/bin/env bash
#
# Regular maintenance performed on a buildkite agent to control disk usage
#


echo --- Delete all exited containers first
(
  set -x
  exited=$(docker ps -aq --no-trunc --filter "status=exited")
  if [[ -n "$exited" ]]; then
    # shellcheck disable=SC2086 # Don't want to double quote "$exited"
    docker rm $exited
  fi
)

echo --- Delete untagged images
(
  set -x
  untagged=$(docker images | grep '<none>'| awk '{ print $3 }')
  if [[ -n "$untagged" ]]; then
    # shellcheck disable=SC2086 # Don't want to double quote "$untagged"
    docker rmi $untagged
  fi
)

echo --- Delete all dangling images
(
  set -x
  dangling=$(docker images --filter 'dangling=true' -q --no-trunc | sort | uniq)
  if [[ -n "$dangling" ]]; then
    # shellcheck disable=SC2086 # Don't want to double quote "$dangling"
    docker rmi $dangling
  fi
)

echo --- Remove unused docker networks
(
  set -x
  docker network prune -f
)

echo "--- Delete /tmp files older than 1 day owned by $(id -un)"
(
  set -x
  find /tmp -maxdepth 1 -user "$(id -un)" -mtime +1 -print0 | xargs -0 rm -rf
)

echo --- Deleting stale buildkite agent build directories
if [[ ! -d ../../../../builds/$BUILDKITE_AGENT_NAME ]]; then
  # We might not be where we think we are, do nothing
  echo Warning: Skipping flush of stale agent build directories
  echo "  PWD=$PWD"
else
  # NOTE: this will be horribly broken if we ever decide to run multiple
  #       agents on the same machine.
  (
    for keepDir in "$BUILDKITE_PIPELINE_SLUG" \
                   "$BUILDKITE_ORGANIZATION_SLUG" \
                   "$BUILDKITE_AGENT_NAME"; do
      cd .. || exit 1
      for dir in *; do
        if [[ -d $dir && $dir != "$keepDir" ]]; then
          echo "Removing $dir"
          rm -rf "${dir:?}"/
        fi
      done
    done
  )
fi

echo --- System Status
(
  set -x
  docker images
  docker ps
  docker network ls
  df -h
)

exit 0
