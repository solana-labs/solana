#!/usr/bin/env bash -e

cd "$(dirname "$0")/.."

_() {
  echo "--- $*"
  "$@"
}

maybe_install() {
  for cmd in "$@"; do
    set +e
    "$cmd" --help > /dev/null 2>&1
    declare exitcode=$?
    set -e
    if [[ $exitcode -ne 0 ]]; then
      _ cargo install "$cmd"
    fi
  done
}

export PATH=$CARGO_HOME/bin:$PATH
maybe_install mdbook
_ mdbook test
_ mdbook build

echo --- create book repo
(
  set -x
  cd book/
  git init .
  git config user.email "maintainers@solana.com"
  git config user.name "$(basename "$0")"
  git commit -m "Initial commit" --allow-empty
  git add ./* ./.nojekyll
  git commit -m "$BUILDKITE_COMMIT"
)

echo --- publish
if [[ $BUILDKITE_BRANCH = master ]]; then
  (
    set -x
    cd book/
    git remote add origin git@github.com:solana-labs/solana.git
    git push -f origin HEAD:gh-pages
  )
else
  echo "Publish skipped"
fi

exit 0
