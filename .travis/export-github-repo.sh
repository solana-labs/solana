#!/usr/bin/env bash
#
# Exports a subdirectory into another github repository
#

set -e
if [[ -z $GITHUB_TOKEN ]]; then
  echo GITHUB_TOKEN not defined
  exit 1
fi

cd "$(dirname "$0")/.."

pip3 install git-filter-repo

declare subdir=$1
declare repo_name=$2

[[ -n "$subdir" ]] || {
  echo "Error: subdir not specified"
  exit 1
}
[[ -n "$repo_name" ]] || {
  echo "Error: repo_name not specified"
  exit 1
}

.travis/affects.sh "^$subdir/" || exit 0
echo "Exporting $subdir"

set -x
rm -rf .github_export/"$repo_name"
git clone https://"$GITHUB_TOKEN"@github.com/solana-labs/"$repo_name" .github_export/"$repo_name"

# TODO: Try using `--refs $TRAVIS_COMMIT_RANGE` to speed up the filtering
git filter-repo --subdirectory-filter "$subdir" --target .github_export/"$repo_name"

git -C .github_export/"$repo_name" push https://"$GITHUB_TOKEN"@github.com/solana-labs/"$repo_name"
