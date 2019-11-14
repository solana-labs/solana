#!/usr/bin/env bash
#
# Checks if a CI build affects one or more path patterns.  Each command-line
# argument is checked in series.
#
# Bash regular expressions are permitted in the pattern:
#     ./affects-files.sh .rs$    -- any file or directory ending in .rs
#     ./affects-files.sh .rs     -- also matches foo.rs.bar
#     ./affects-files.sh ^snap/  -- anything under the snap/ subdirectory
#     ./affects-files.sh snap/   -- also matches foo/snap/
# Any pattern starting with the ! character will be negated:
#     ./affects-files.sh !^book/  -- anything *not* under the book/ subdirectory
#
set -e
cd "$(dirname "$0")"/..

if [[ -n $CI_PULL_REQUEST ]]; then
  affectedFiles="$(buildkite-agent meta-data get affected_files)"
  echo "Affected files in this PR: $affectedFiles"

  IFS=':' read -ra files <<< "$affectedFiles"
  for pattern in "$@"; do
    if [[ ${pattern:0:1} = "!" ]]; then
      for file in "${files[@]}"; do
        if [[ ! $file =~ ${pattern:1} ]]; then
          exit 0
        fi
      done
    else
      for file in "${files[@]}"; do
        if [[ $file =~ $pattern ]]; then
          exit 0
        fi
      done
    fi
  done

  exit 1
fi

# affected_files metadata is not currently available for non-PR builds, so assume
# the worse (affected)
exit 0
