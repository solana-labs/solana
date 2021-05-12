---
title: Solana Cluster RPCエンドポイント
---

Solana は [JSON-RPC](developing/clients/jsonrpc-api.md) の各パブリッククラスタへの要求を満たすために専用の api ノードを維持しており、サードパーティも同様です。 現在利用可能なパブリック RPC エンドポイントと、各パブリッククラスターで推奨される RPC エンドポイントをご紹介します。

## Devnet

#### Endpoint

- `https://devnet.solana.com` - single Solana-hosted api node; rate limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## テストネット

#### Endpoint

- `https://testnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## メインネットベータ

#### Endpoints

- `https://api.mainnet-beta.solana.com` - Solana-hosted api node cluster, backed by a load balancer; rate-limited
- `https://solana-api.projectserum.com` - Project Serum-hosted api node

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB
