#!/usr/bin/env bash

set -e

if [[ -d .vercel ]]; then
  rm -r .vercel
fi

CONFIG_FILE=vercel.json

if [[ -n $CI_TAG ]]; then
  PROJECT_NAME=docs-solana-com
else
  eval "$(../ci/channel-info.sh)"
  case $CHANNEL in
  edge)
    PROJECT_NAME=edge-docs-solana-com
    ;;
  beta)
    PROJECT_NAME=beta-docs-solana-com
    ;;
  *)
    PROJECT_NAME=docs
    ;;
  esac
fi

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
vercel deploy . --local-config="$CONFIG_FILE" --confirm --token "$VERCEL_TOKEN" --prod
