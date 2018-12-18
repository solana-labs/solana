#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

book/build.sh

echo --- create book repo
(
  set -x
  cd book/html/
  git init .
  git config user.email "maintainers@solana.com"
  git config user.name "$(basename "$0")"
  git add ./* ./.nojekyll
  git commit -m "${BUILDKITE_COMMIT:-local}"
)

echo --- publish
if [[ $BUILDKITE_BRANCH = master ]]; then
  cd book/html/
  git remote add origin git@github.com:solana-labs/solana.git
  git fetch origin gh-pages
  if ! git diff HEAD origin/gh-pages --quiet; then
    git push -f origin HEAD:gh-pages
  else
    echo "Content unchanged, publish skipped"
  fi
else
  echo "Publish skipped"
fi

exit 0
