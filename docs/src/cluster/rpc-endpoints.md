---
title: Solana Cluster RPC Endpoints
---

Solana maintains dedicated api nodes to fulfill [JSON-RPC](/api)
requests for each public cluster, and third parties may as well. Here are the
public RPC endpoints currently available and recommended for each public cluster:

## Devnet

#### Endpoint

- `https://api.devnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Testnet

#### Endpoint

- `https://api.testnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Mainnet Beta

#### Endpoints\*

- `https://api.mainnet-beta.solana.com` - Solana-hosted api node cluster, backed by a load balancer; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum concurrent connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

\*The public RPC endpoints are not intended for production applications. Please
use dedicated/private RPC servers when you launch your application, drop NFTs,
etc. The public services are subject to abuse and rate limits may change
without prior notice. Likewise, high-traffic websites may be blocked without
prior notice.

## Common HTTP Error Codes

- 403 -- Your IP address or website has been blocked. It is time to run your own RPC server(s) or find a private service.
- 429 -- Your IP address is exceeding the rate limits. Slow down! Use the
  [Retry-After](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Retry-After)
  HTTP response header to determine how long to wait before making another
  request.
