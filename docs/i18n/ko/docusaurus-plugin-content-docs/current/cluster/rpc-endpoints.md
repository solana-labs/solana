---
title: 솔라나 클러스터 RPC 엔드포인트
---

솔라나는 각 공용 클러스터에 대한[JSON-RPC](developing/clients/jsonrpc-api.md) 요청을 수행하기 위해 전용 API 노드를 유지하며, 이는 제 3자도 가능합니다. 다음은 현재 사용 가능하고 각 공용 클러스터에 권장되는 공용 RPC 엔드포인트 입니다.

## Devnet

#### Endpoint

- `https://devnet.solana.com` - 단일 솔라나 호스팅 API 노드; 제한적 수수료

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Testnet

#### Endpoint

- `https://testnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## Mainnet Beta

#### Endpoints

- `https://api.mainnet-beta.solana.com` - Solana-hosted api node cluster, backed by a load balancer; rate-limited
- `https://solana-api.projectserum.com` - Project Serum-hosted api node

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB
