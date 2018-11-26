#!/usr/bin/env bash
set -e

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
maybe_install svgbob_cli
_ make -C book

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
  (
    set -x
    cd book/html/
    git remote add origin git@github.com:solana-labs/solana.git
    git push -f origin HEAD:gh-pages
  )
else
  echo "Publish skipped"
fi

exit 0
