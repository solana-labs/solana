#!/usr/bin/env bash
#
# Checks if a CI build affects one or more path patterns.  Each command-line
# argument is checked in series.
#
# Bash regular expresses are permitted in the pattern, and the colon (':')
# character is guaranteed to appear before and after each affected file (and thus
# can be used as an anchor), eg:
#     .rs:    -- any file or directory ending in .rs
#     .rs     -- also matches foo.rs.bar
#     :snap/  -- anything under the snap/ subdirectory
#     snap/   -- also matches foo/snap/
#

if ci/is-pr.sh; then
  affectedFiles="$(buildkite-agent meta-data get affected_files)"
  echo "Affected files in this PR: $affectedFiles"

  for pattern in "$@"; do
    if [[ ":$affectedFiles:" =~ $pattern ]]; then
      exit 0
    fi
  done

  exit 1
fi

# affected_files metadata is not currently available for non-PR builds, so assume
# the worse (affected)
exit 0
