#!/usr/bin/env bash
set -e
#
# Only run snap.sh for pull requests that modify files under /snap
#

cd "$(dirname "$0")"/..

ci/affects-files.sh :snap/ || {
  echo "Skipping snap build as no files under /snap were modified"
  exit 0
}

exec ci/snap.sh
