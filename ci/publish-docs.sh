#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

me=$(basename "$0")

echo --- update gitbook-cage
if [[ -n $CI_BRANCH ]]; then
  (
    set -x
    (
      . ci/rust-version.sh stable
      ci/docker-run.sh "$rust_stable_docker_image" make -C docs
    )
    # make a local commit for the svgs
    git add -A -f docs/src/.gitbook/assets/.
    if ! git diff-index --quiet HEAD; then
      git config user.email maintainers@solana.com
      git config user.name "$me"
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
