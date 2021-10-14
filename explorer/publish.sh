#!/usr/bin/env bash

set -ex

if [[ -d .vercel ]]; then
  rm -r .vercel
fi

CONFIG_FILE=vercel.json
PROJECT_NAME=explorer

cat > "$CONFIG_FILE" <<EOF
{
  "name": "$PROJECT_NAME",
  "scope": "solana-labs"
}
EOF

[[ -n $VERCEL_TOKEN ]] || {
  echo "VERCEL_TOKEN is undefined.  Needed for Vercel authentication."
  exit 1
}
vercel deploy . --local-config="$CONFIG_FILE" --confirm --token "$VERCEL_TOKEN"

