---
title: オフライン取引署名
---

いくつかのセキュリティモデルでは、署名キー、つまり署名プロセスを、トランザクションの作成やネットワークのブロードキャストから分離しておく必要があります。 その例を以下に挙げます。

- [マルチシグネチャスキーム](cli/usage.md#multiple-witnesses)で地理的に離れた署名者の署名を収集します。
- [エアギャップ](<https://en.wikipedia.org/wiki/Air_gap_(networking)>)を利用した署名装置を使ったトランザクションの署名します。

この文書では、Solana の CLI を使用して別々に署名してトランザクションを送信する方法について説明します。

## オフラインサインをサポートするコマンド

現時点では、以下のコマンドがオフライン署名に対応しています。

- [`create-stake-account`](cli/usage.md#solana-create-stake-account)
- [`deactivate-stake`](cli/usage.md#solana-deactivate-stake)
- [`delegate-stake`](cli/usage.md#solana-delegate-stake)
- [`split-stake`](cli/usage.md#solana-split-stake)
- [`stake-authorize`](cli/usage.md#solana-stake-authorize)
- [`stake-set-lockup`](cli/usage.md#solana-stake-set-lockup)
- [`transfer`](cli/usage.md#solana-transfer)
- [`withdraw-stake`](cli/usage.md#solana-withdraw-stake)

## トランザクションをオフラインで署名しよう

トランザクションをオフラインで署名するには、コマンドラインで次の引数を渡します。

1. `"-sign-only"`は、クライアントが署名されたトランザクションをネットワークに送信しないようにします。 代わりに、"Pubkey/Signature"のペアが標準出力に出力されます。
2. `"--blockhash BASE58_HASH"`は、呼び出し側がトランザクションの`"recent_blockhash`フィールドを埋めるために使用される値を指定できるようにします。 これには以下のような目的があります。 _ネットワークに接続して RPC 経由で最近のブロックハッシュを照会する必要性を排除すること。 _ 複数の署名スキームで署名者がブロックハッシュを調整できるようにすること。

### 例 オフラインでの支払いの署名

コマンド

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipent-keypair.json 1
```

出力

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## オフラインで署名されたトランザクションをネットワークに送信しよう

オフラインで署名されたトランザクションをネットワークに送信するには、コマンドラインで次の引数を渡します。

1. `"--blockhash BASE58_HASH"`、署名に使用されたものと同じブロックハッシュでなければなりません。
2. `"-signer BASE58_PUBKEY=BASE58_SIGNATURE"`、各オフライン署名者に 1 つずつ必要です。 これは、ローカルキーペアで署名するのではなく、Pubkey/Signature ペア(s) を直接トランザクションに含めるものです。

### 例 オフラインでの署名入り支払いの提出

コマンド

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

出力

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## 複数のセッションでのオフライン署名

オフライン署名は、複数のセッションにわたって行われることもあります。 このシナリオでは、不在の署名者の公開キーを各ロールに渡します。 指定されたものの、署名が生成されなかったすべての公開キーは、オフライン署名の出力に"absent"と表示されます。

### 例 2 つのオフラインサインセッションでの転送

コマンド(オフラインセッション#1)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut, 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKKXyjAwwENH9GomyM8Dbc \
    --signonly \
    ---keypair fee_payer.json \
    --from 674RgFmgFgdqSBg7mHFbrrNm1h721H1r721HMquHL
```

出力(オフラインセッション#1)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

コマンド(オフラインセッション#2)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut, 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKKXyjAwwENH9GomyM8Dbc \
    --signonly \
    ---keypair fee_payer.json \
    --from 674RgFmgFgdqSBg7mHFbrrNm1h721H1r721HMquHL
```

出力(オフラインセッション#2)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

コマンド (オンラインサブミッション)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

出力(オンラインサブミッション)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## サインのための時間を増やします。

通常、Solana のトランザクションは、`recent_blockhash`フィールドのブロックハッシュからいくつかのスロット内で署名され、ネットワークに受け入れられなければなりません(この記事を書いている時点では～ 2 分) 署名手続きにこれ以上の時間がかかる場合は、["Durable Transaction Nonce"](offline-signing/durable-nonce.md)を使うことで必要な時間を確保することができます。
