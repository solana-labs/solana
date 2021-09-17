#!/usr/bin/env bash

set -ex

cd "$(dirname "$0")/.."

if ! echo "$BUILDKITE_BRANCH" | grep -E '^pull/[0-9]+/head$'; then
  echo "not pull request!?" >&2
  exit 1
fi

source ci/rust-version.sh stable

ci/docker-run.sh $rust_nightly_docker_image ci/dependabot-updater.sh

if [[ $(git status --short :**/Cargo.lock | wc -l) -eq 0 ]]; then
  echo --- ok
  exit 0
fi

echo --- "(FAILING) Backpropagating dependabot-triggered Cargo.lock updates"

name="dependabot-buildkite"
api_base="https://api.github.com/repos/solana-labs/solana/pulls"
pr_num=$(echo "$BUILDKITE_BRANCH" | grep -Eo '[0-9]+')
branch=$(curl -s "$api_base/$pr_num" | python3 -c 'import json,sys;print(json.load(sys.stdin)["head"]["ref"])')

git add :**/Cargo.lock
EMAIL="dependabot-buildkite@noreply.solana.com" \
  GIT_AUTHOR_NAME="$name" \
  GIT_COMMITTER_NAME="$name" \
  git commit -m "[auto-commit] Update all Cargo lock files"
git push origin "HEAD:$branch"

echo "Source branch is updated; failing this build for the next"
exit 1
