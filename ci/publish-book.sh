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

if [[ "$MANUAL_PUBLISH" = true ]]; then
  case $BOOK in
  book)
    repo=git@github.com:solana-labs/book.git
    ;;
  book-edge)
    repo=git@github.com:solana-labs/book-edge.git
    ;;
  book-beta)
    repo=git@github.com:solana-labs/book-beta.git
    ;;
  *)
   echo "--- unknown book: $BOOK"
   exit 1
   ;;
  esac
else
  eval "$(ci/channel-info.sh)"
  # Only publish the book from the edge and beta channels for now.
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
