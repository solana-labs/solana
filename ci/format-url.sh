#!/usr/bin/env bash
#
# Formats a URL to be clickable from a Buildkite log
#

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 url"
  exit 1
fi

if [[ -z $BUILDKITE ]]; then
  echo "$1"
else
  # shellcheck disable=SC2001
  URL="$(echo "$1" | sed 's/;/%3b/g')" # Escape ;

  printf '\033]1339;url='
  echo -n "$URL"
  printf '\a\n'
fi
