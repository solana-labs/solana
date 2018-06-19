#!/bin/bash
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

echo "--- Delete /tmp files older than 1 day owned by $(whoami)"
(
  set -x
  find /tmp -maxdepth 1 -user "$(whoami)" -mtime +1 -print0 | xargs -0 rm -rf
)

echo --- System Status
(
  set -x
  docker images
  docker ps
  docker network ls
  df -h
)

exit 0
