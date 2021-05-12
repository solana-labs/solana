---
title: نقاط نهاية RPC مجموعة Solana
---

تحتفظ Solana بعُقَد (nodes) مُخصصة على واجهة برمجة التطبيقات (API) لتلبية [JSON-RPC](developing/clients/jsonrpc-api.md) طلبات كل مجموعة عامة (public cluster)، ويُمكن للأطراف الثالثة أيضا. فيما يلي نقاط نهاية إجراء الإتصال عن بعد العامة (public RPC endpoints) المُتاحة حاليا والمُوصى بها لكل مجموعة عامة (public cluster):

## شبكة المُطوِّرين (Devnet)

#### Endpoint

- `https://devnet.solana.com` - عُقدة واجهة برمجة التطبيقات (api node) واحدة مُستضافة من قِبَل Solana؛ محدود السعر

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## الشبكة التجريبية (tesnet)

#### Endpoint

- `https://testnet.solana.com` - single Solana-hosted api node; rate-limited

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB

## الشبكة التجريبية الرئيسية (mainnet beta)

#### Endpoints

- `https://api.mainnet-beta.solana.com` - Solana-hosted api node cluster, backed by a load balancer; rate-limited
- `https://solana-api.projectserum.com` - Project Serum-hosted api node

#### Rate Limits

- Maximum number of requests per 10 seconds per IP: 100
- Maximum number of requests per 10 seconds per IP for a single RPC: 40
- Maximum current connections per IP: 40
- Maximum connection rate per 10 seconds per IP: 40
- Maximum amount of data per 30 second: 100 MB
