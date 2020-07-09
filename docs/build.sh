#!/usr/bin/env bash
set -ex

source ../ci/env.sh

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
npm run build

# Deploy the /build content using vercel
if [[ -d .vercel ]]; then
  rm -r .vercel
fi
./set-vercel-project-name.sh

if [[ -n $CI ]]; then
  if [[ -z $CI_PULL_REQUEST ]]; then
    [[ -n $VERCEL_TOKEN ]] || {
      echo "VERCEL_TOKEN is undefined.  Needed for Vercel authentication."
      exit 1
    }
    vercel deploy . --local-config=vercel.json --confirm --token "$VERCEL_TOKEN" --prod
  fi
else
  vercel deploy . --local-config=vercel.json
fi
