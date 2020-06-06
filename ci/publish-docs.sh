#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

echo --- build docs
(
  set -x
  . ci/rust-version.sh stable
  ci/docker-run.sh "$rust_stable_docker_image" docs/build.sh
)

echo --- update gitbook-cage
if [[ -n $CI_BRANCH ]]; then
  (
    # make a local commit for the svgs and generated/updated markdown
    set -x
    git add -f docs/src
    if ! git diff-index --quiet HEAD; then
      git config user.email maintainers@solana.foundation
      git config user.name "$(basename "$0")"
      git commit -m "gitbook-cage update $(date -Is)"
      git push -f git@github.com:solana-labs/solana-gitbook-cage.git HEAD:refs/heads/"$CI_BRANCH"
      # pop off the local commit
      git reset --hard HEAD~
    fi
  )
else
  echo CI_BRANCH not set
fi

exit 0
