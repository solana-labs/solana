#!/usr/bin/env bash

set -ex

cd "$(dirname "$0")/.."

if ! echo "$BUILDKITE_BRANCH" | grep -E '^pull/[0-9]+/head$'; then
  echo "not pull request!?"
  exit 1
fi

source ci/rust-version.sh stable

ci/docker-run.sh $rust_nightly_docker_image ci/dependabot-updater.sh

if [[ $(git status --short :**/Cargo.lock | wc -l) -gt 0 ]]; then
  (
    echo --- "(FAILING) Backpropagating dependabot-triggered Cargo.lock updates"

    git add :**/Cargo.lock
    NAME="dependabot-buildkite"
    GIT_AUTHOR_NAME="$NAME" \
      GIT_COMMITTER_NAME="$NAME" \
      EMAIL="dependabot-buildkite@noreply.solana.com" \
      git commit -m "[auto-commit] Update all Cargo lock files"
    api_base="https://api.github.com/repos/solana-labs/solana/pulls"
    pr_num=$(echo "$BUILDKITE_BRANCH" | grep -Eo '[0-9]+')
    branch=$(curl -s "$api_base/$pr_num" | python -c 'import json,sys;print json.load(sys.stdin)["head"]["ref"]')
    git push origin "HEAD:$branch"

    echo "Source branch is updated; failing this build for the next"
    exit 1
  )
fi

echo --- ok
