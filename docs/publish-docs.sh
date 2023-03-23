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
  "scope": "$VERCEL_SCOPE",
  "redirects": [
    { "source": "/apps", "destination": "/developers" },
    { "source": "/developing/programming-model/overview", "destination": "/developers" },
    { "source": "/apps/backwards-compatibility", "destination": "/developing/backwards-compatibility" },
    { "source": "/apps/break", "destination": "/developing/on-chain-programs/examples" },
    { "source": "/apps/builtins", "destination": "/developing/runtime-facilities/programs" },
    { "source": "/apps/drones", "destination": "/developing/on-chain-programs/examples" },
    { "source": "/apps/hello-world", "destination": "/developing/on-chain-programs/examples" },
    { "source": "/apps/javascript-api", "destination": "/developing/clients/javascript-api" },
    { "source": "/apps/jsonrpc-api", "destination": "/api/http" },
    { "source": "/developing/clients/jsonrpc-api", "destination": "/api/http" },
    { "source": "/apps/programming-faq", "destination": "/developing/on-chain-programs/faq" },
    { "source": "/apps/rent", "destination": "/developing/programming-model/accounts#rent" },
    { "source": "/apps/sysvars", "destination": "/developing/runtime-facilities/sysvars" },
    { "source": "/apps/webwallet", "destination": "/wallet-guide" },
    { "source": "/implemented-proposals/cross-program-invocation", "destination": "/developing/programming-model/calling-between-programs" },
    { "source": "/implemented-proposals/program-derived-addresses", "destination": "/developing/programming-model/calling-between-programs#program-derived-addresses" },
    { "source": "/implemented-proposals/secp256k1_instruction", "destination": "/developing/runtime-facilities/programs#secp256k1-program" }
  ]
}
EOF

[[ -n $VERCEL_TOKEN ]] || {
  echo "VERCEL_TOKEN is undefined.  Needed for Vercel authentication."
  exit 1
}
vercel deploy . --local-config="$CONFIG_FILE" --confirm --token "$VERCEL_TOKEN" --prod
