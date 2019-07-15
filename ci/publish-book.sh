#!/usr/bin/env bash
set -e

# Allow release tag from cmd line
if [[ -n $1 ]] ; then
  PUBLISH_BOOK_TAG=$1
fi

cd "$(dirname "$0")/.."
BOOK="book"

if [[ -n $PUBLISH_BOOK_TAG ]]; then
  # book is manually published at a specified release tag
  repo=git@github.com:solana-labs/book.git
  git checkout "${PUBLISH_BOOK_TAG}" book
else
  # book-edge and book-beta are published automatically on the tip of the branch
  eval "$(ci/channel-info.sh)"
  case $CHANNEL in
  edge)
    repo=git@github.com:solana-labs/book-edge.git
    ;;
  beta)
    repo=git@github.com:solana-labs/book-beta.git
    ;;
  *)
   echo "--- publish skipped"
   exit 0
   ;;
  esac
  BOOK=$CHANNEL
fi

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

echo "--- publish $BOOK"
cd book/html/
git remote add origin $repo
git fetch origin master
if ! git diff HEAD origin/master --quiet; then
  git push -f origin HEAD:master
else
  echo "Content unchanged, publish skipped"
fi

exit 0
