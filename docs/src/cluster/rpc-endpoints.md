---
title: Safecoin Cluster RPC Endpoints
---

Safecoin maintains dedicated api nodes to fulfill [JSON-RPC](developing/clients/jsonrpc-api.md)
requests for each public cluster, and third parties may as well. Here are the
public RPC endpoints currently available and recommended for each public cluster:

## Devnet

- `https://devnet.safecoin.org` - single Safecoin-hosted api node; rate-limited

## Testnet

- `https://testnet.safecoin.org` - single Safecoin-hosted api node; rate-limited

## Mainnet Beta

- `https://api.mainnet-beta.safecoin.org` - Safecoin-hosted api node cluster, backed by a load balancer; rate-limited
- `https://solana-api.projectserum.com` - Project Serum-hosted api node
