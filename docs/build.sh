#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

# md check
find src -name '*.md' -a \! -name SUMMARY.md |
 while read -r file; do
   if ! grep -q '('"${file#src/}"')' src/SUMMARY.md; then
       echo "Error: $file missing from SUMMARY.md"
       exit 1
   fi
 done

# auto-generate src/cli/usage.md
./build-cli-usage.sh

./set-solana-release-tag.sh

# Build from /src into /build
yarn
yarn build

# Deploy the /build content using vercel
yarn global add vercel
if [[ -d .vercel ]]; then
  rm -r .vercel
fi

PROD=
if [[ -n $CI ]]; then
  if [[ -n $TOKEN ]]; then
    TOKEN_OPT="--token $TOKEN"
  else
    echo "TOKEN is undefined.  Needed for Vercel authentication."
    exit 1
  fi

  # Only push to production domains for non-PR jobs, otherwise just staging
  if [[ -z $CI_PULL_REQUEST ]]; then
    PROD="--prod"
  fi
fi

./set-vercel-project-name.sh

vercel deploy . --local-config=vercel.json --confirm "$TOKEN_OPT" "$PROD"
