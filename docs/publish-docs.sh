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
    { "source": "/developing/programming-model/overview", "destination": "https://solana.com/docs/programs" },
    { "source": "/apps/break", "destination": "https://solana.com/docs/programs/examples" },
    { "source": "/apps/drones", "destination": "https://solana.com/docs/programs/examples" },
    { "source": "/apps/hello-world", "destination": "https://solana.com/docs/programs/examples" },
    { "source": "/apps/javascript-api", "destination": "https://solana.com/docs/clients/javascript" },
    { "source": "/apps/programming-faq", "destination": "https://solana.com/docs/programs/faq" },
    { "source": "/apps/rent", "destination": "https://solana.com/docs/core/rent" },
    { "source": "/apps/webwallet", "destination": "https://solana.com/docs/intro/wallets" },
    { "source": "/implemented-proposals/cross-program-invocation", "destination": "https://solana.com/docs/core/cpi" },
    { "source": "/implemented-proposals/program-derived-addresses", "destination": "https://solana.com/docs/core/cpi#program-derived-addresses" },
    { "source": "/apps/jsonrpc-api", "destination": "/api/http" },
    { "source": "/developing/clients/jsonrpc-api", "destination": "/api/http" },
    { "source": "/apps/sysvars", "destination": "/developing/runtime-facilities/sysvars" },
    { "source": "/apps/builtins", "destination": "/developing/runtime-facilities/programs" },
    { "source": "/apps/backwards-compatibility", "destination": "/developing/backwards-compatibility" },
    { "source": "/implemented-proposals/secp256k1_instruction", "destination": "/developing/runtime-facilities/programs#secp256k1-program" },
    { "destination": "https://solana.com/docs/terminology", "source": "/terminology" },
    { "destination": "https://solana.com/docs/core/rent", "source": "/developing/intro/rent" },
    { "destination": "https://solana.com/docs/core/programs", "source": "/developing/intro/programs" },
    { "destination": "https://solana.com/docs/core/accounts", "source": "/developing/programming-model/accounts" },
    { "destination": "https://solana.com/docs/core/cpi", "source": "/developing/programming-model/calling-between-programs" },
    { "destination": "https://solana.com/docs/core/runtime", "source": "/developing/programming-model/runtime" },
    { "destination": "https://solana.com/docs/core/transactions", "source": "/developing/programming-model/transactions" },
    { "destination": "https://solana.com/docs/core/transactions/fees", "source": "/developing/intro/transaction_fees" },
    { "destination": "https://solana.com/docs/core/transactions/confirmation", "source": "/developing/transaction_confirmation" },
    { "destination": "https://solana.com/docs/core/transactions/versions", "source": "/developing/versioned-transactions" },
    { "destination": "https://solana.com/docs/core/transactions/retry", "source": "/integrations/retrying-transactions" },
    { "destination": "https://solana.com/docs/intro/dev", "source": "/developing/programming-model/overview" },
    { "destination": "https://solana.com/docs/advanced/lookup-tables", "source": "/developing/lookup-tables" },
    { "destination": "https://solana.com/docs/advanced/state-compression", "source": "/learn/state-compression" },
    { "destination": "https://solana.com/developers/guides/javascript/compressed-nfts", "source": "/developing/guides/compressed-nfts" },
    { "destination": "https://solana.com/docs/programs", "source": "/developing/on-chain-programs/overview" },
    { "destination": "https://solana.com/docs/programs/debugging", "source": "/developing/on-chain-programs/debugging" },
    { "destination": "https://solana.com/docs/programs/deploying", "source": "/developing/on-chain-programs/deploying" },
    { "destination": "https://solana.com/docs/programs/examples", "source": "/developing/on-chain-programs/examples" },
    { "destination": "https://solana.com/docs/programs/faq", "source": "/developing/on-chain-programs/faq" },
    { "destination": "https://solana.com/docs/programs/limitations", "source": "/developing/on-chain-programs/limitations" },
    { "destination": "https://solana.com/docs/programs/lang-rust", "source": "/developing/on-chain-programs/developing-rust" },
    { "destination": "https://solana.com/docs/programs/lang-c", "source": "/developing/on-chain-programs/developing-c" },
    { "destination": "https://solana.com/docs/clients/javascript-reference", "source": "/developing/clients/javascript-reference" },
    { "destination": "https://solana.com/docs/clients/javascript", "source": "/developing/clients/javascript-api" },
    { "destination": "https://solana.com/docs/clients/rust", "source": "/developing/clients/rust-api" },
    { "destination": "https://solana.com/docs/intro/dev", "source": "/getstarted/overview" },
    { "destination": "https://solana.com/developers/guides/getstarted/hello-world-in-your-browser", "source": "/getstarted/hello-world" },
    { "destination": "https://solana.com/developers/guides/getstarted/setup-local-development", "source": "/getstarted/local" },
    { "destination": "https://solana.com/developers/guides/getstarted/local-rust-hello-world", "source": "/getstarted/rust" },
    { "destination": "https://solana.com/docs/core/clusters", "source": "/clusters/rpc-endpoints" },
    { "destination": "https://solana.com/docs/economics/staking", "source": "/staking" },
    { "destination": "https://solana.com/docs/economics/staking/:path*", "source": "/staking/:path*" },
    { "destination": "https://solana.com/docs/economics/inflation/:path*", "source": "/inflation/:path*" },
    { "destination": "https://solana.com/docs/more/exchange", "source": "/integrations/exchange" },
    { "destination": "https://solana.com/docs/intro/transaction_fees", "source": "/transaction_fees" },
    { "destination": "https://solana.com/docs/intro/economics", "source": "/storage_rent_economics" },
    { "destination": "https://solana.com/docs/intro/economics", "source": "/economics_overview" },
    { "destination": "https://solana.com/docs/intro/history", "source": "/history" },
    { "destination": "https://solana.com/docs/intro/wallets", "source": "/wallet-guide/support" },
    { "destination": "https://solana.com/docs/intro/wallets", "source": "/wallet-guide" },
    { "destination": "https://solana.com/docs/intro", "source": "/introduction" }
  ]
}
EOF

[[ -n $VERCEL_TOKEN ]] || {
  echo "VERCEL_TOKEN is undefined.  Needed for Vercel authentication."
  exit 1
}
vercel deploy . --local-config="$CONFIG_FILE" --confirm --token "$VERCEL_TOKEN" --prod
