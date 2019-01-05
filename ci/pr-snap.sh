#!/usr/bin/env bash
#
# Only run publish-snap.sh for pull requests that modify files under /snap
#
set -e
cd "$(dirname "$0")"/..

ci/affects-files.sh ^snap/ || {
  echo "Skipping snap build as no files under /snap were modified"
  exit 0
}

exec ci/publish-snap.sh
