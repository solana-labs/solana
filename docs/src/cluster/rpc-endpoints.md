---
title: Solana Cluster RPC Endpoints
---

Solana maintains dedicated api nodes to fulfill [JSON-RPC](developing/clients/jsonrpc-api.md)
requests for each public cluster, and third parties may as well. Here are the
public RPC endpoints currently available and recommended for each public cluster:

## Devnet {#devnet}

#### Endpoint {#endpoint}

- `https://api.devnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits {#rate-limits}

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Testnet {#testnet}

#### Endpoint {#endpoint-1}

- `https://api.testnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits {#rate-limits-1}

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Mainnet Beta {#mainnet-beta}

#### Endpoints {#endpoints}

- `https://api.mainnet-beta.solana.com` - Solana-hosted api node cluster, backed by a load balancer; rate-limited
- `https://solana-api.projectserum.com` - Project Serum-hosted api node

#### Rate Limits {#rate-limits-2}

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB
