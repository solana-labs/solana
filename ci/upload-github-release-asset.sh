#!/usr/bin/env bash
#
# Uploads one or more files to a github release
#
# Prerequisites
# 1) GITHUB_TOKEN defined in the environment
# 2) TAG defined in the environment
#
set -e

if [[ -z $1 ]]; then
  echo No files specified
  exit 1
fi

if [[ -z $GITHUB_TOKEN ]]; then
  echo Error: GITHUB_TOKEN not defined
  exit 1
fi

if [[ -z $CI_TAG ]]; then
  echo Error: CI_TAG not defined
  exit 1
fi

if [[ -z $CI_REPO_SLUG ]]; then
  echo Error: CI_REPO_SLUG not defined
  exit 1
fi

releaseId=$( \
  curl -s "https://api.github.com/repos/$CI_REPO_SLUG/releases/tags/$CI_TAG" \
  | grep -m 1 \"id\": \
  | sed -ne 's/^[^0-9]*\([0-9]*\),$/\1/p' \
)
echo "Github release id for $CI_TAG is $releaseId"

for file in "$@"; do
  echo "--- Uploading $file to tag $CI_TAG of $CI_REPO_SLUG"
  curl \
    --data-binary @"$file" \
    -H "Authorization: token $GITHUB_TOKEN" \
    -H "Content-Type: application/octet-stream" \
    "https://uploads.github.com/repos/$CI_REPO_SLUG/releases/$releaseId/assets?name=$(basename "$file")"
  echo
done

