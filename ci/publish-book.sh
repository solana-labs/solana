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
  git commit -m "${CI_COMMIT:-local}"
)

eval "$(ci/channel-info.sh)"
# Only publish the book from the edge and beta channels for now.
case $CHANNEL in
edge)
  repo=git@github.com:solana-labs/book-edge.git
  ;;
beta)
  repo=git@github.com:solana-labs/book.git
  ;;
*)
 echo "--- publish skipped"
 exit 0
 ;;
esac

echo "--- publish $CHANNEL"
cd book/html/
git remote add origin $repo
git fetch origin master
if ! git diff HEAD origin/master --quiet; then
  git push -f origin HEAD:master
else
  echo "Content unchanged, publish skipped"
fi

exit 0
