---
title: Solana Cluster RPCエンドポイント
---

Solana は [JSON-RPC](developing/clients/jsonrpc-api.md) の各パブリッククラスタへの要求を満たすために専用の api ノードを維持しており、サードパーティも同様です。 現在利用可能なパブリック RPC エンドポイントと、各パブリッククラスターで推奨される RPC エンドポイントをご紹介します。

## Devnet

- `https://devnet.solana.com` - single Solana-hosted api node; rate limited

## テストネット

- `https://testnet.solana.com` - single Solana-hosted api node; rate limited

## メインネットベータ

- `https://api.mainnet-beta.solana.com` - Solana がホストする"api ノードクラスター"、"ロードバランサーによるバックアップ"、レート制限あり
- `https://solana-api.projectserum.com` - "Project Serum" がホストする"api ノード"
