#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_

# Look for failed mergify.io backports
_ git show HEAD --check --oneline

_ ci/nits.sh
_ ci/check-ssh-keys.sh

echo --- ok
