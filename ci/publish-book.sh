#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

me=$(basename "$0")

BOOK="book"

source ci/rust-version.sh stable
eval "$(ci/channel-info.sh)"

if [[ -n $PUBLISH_BOOK_TAG ]]; then
  CURRENT_TAG="$(git describe --tags)"
  COMMIT_TO_PUBLISH="$(git rev-list -n 1 "${PUBLISH_BOOK_TAG}")"

  # book is manually published at a specified release tag
  if [[ $PUBLISH_BOOK_TAG != "$CURRENT_TAG" ]]; then
    (
      cat <<EOF
steps:
  - trigger: "$BUILDKITE_PIPELINE_SLUG"
    async: true
    build:
      message: "$BUILDKITE_MESSAGE"
      commit: "$COMMIT_TO_PUBLISH"
      env:
        PUBLISH_BOOK_TAG: "$PUBLISH_BOOK_TAG"
EOF
    ) | buildkite-agent pipeline upload
    exit 0
  fi
  repo=git@github.com:solana-labs/book.git
else
  # book-edge and book-beta are published automatically on the tip of the branch
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

ci/docker-run.sh "$rust_stable_docker_image" bash -exc "book/build.sh"

echo --- create book repo
(
  set -x
  cd book/html/
  git init .
  git add ./* ./.nojekyll
  git commit --author="$me <maintainers@solana.com>" -m "${CI_COMMIT:-local}"
)

echo "--- publish $BOOK"
(
  cd book/html/
  git remote add origin $repo
  git fetch origin master
  if ! git diff HEAD origin/master --quiet; then
    git push -f origin HEAD:master
  else
    echo "Content unchanged, publish skipped"
  fi
)

echo --- update gitbook-cage
(
  if [[ -z $CI_BRANCH ]]; then
    exit 0
  fi

  set -x
  (
    . ci/rust-version.sh
    ci/docker-run.sh $rust_stable_docker_image make -Cbook -B svg
  )
  # make a local commit for the svgs
  git add -A -f book/src/.gitbook/assets/.
  if ! git diff-index --quiet HEAD; then
    git commit --author="$me <maintainers@solana.com>" -m "gitbook-cage update $(date -Is)"
    git push -f github.com:solana-labs/solana-gitbook-cage.git HEAD:refs/heads/"$CI_BRANCH"
    # pop off the local commit
    git reset --hard HEAD~
  fi
)

exit 0
