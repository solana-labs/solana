---
title: 耐久性のあるトランザクションNonces
---

"耐久性のあるトランザクションnonces"は、トランザクションの[`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash)の典型的な短い寿命を回避するためのメカニズムです。 これはSolanaプログラムとして実装されており、その仕組みについては[提案書](../implemented-proposals/durable-tx-nonces.md)を読んでください。

## 使用例

耐久性のある nonce CLI コマンドの完全な使用方法の詳細は [CLI リファレンス](../cli/usage.md) にあります。

### Nonce 権限

"nonceアカウント"の権限は、任意で別のアカウントに割り当てることができます。 そうすることで、新しい権限は、アカウント作成者を含む、前の権限からnonceアカウントに対する完全な制御を継承します。 この機能により、より複雑なアカウント所有権の取り決めや、キーペアに関連付けられていない派生アカウントアドレスの作成が可能になります。 The `--nonce-authority <AUTHORITY_KEYPAIR>` argument is used to specify this account and is supported by the following commands

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### ノンスアカウントの作成

耐久性のあるトランザクションnonce機能では、次のnonce値を保存するためにアカウントを使用します。 耐久性のあるnonceアカウントは[賃料免除でなければならないため](../implemented-proposals/rent.md#two-tiered-rent-regime)、これを実現するためには最低残高が必要です。

"nonce アカウント"は最初に新しいキーペアを生成し、次にチェーン上にアカウントを作成します。

- Command

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- 出力

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> キーペアを完全にオフラインにするには、代わりに["Paper Walle"](wallet-guide/paper-wallet.md)tのキーペア生成[手順](wallet-guide/paper-wallet.md#seed-phrase-generation)を使用します。

> [完全な使用方法ドキュメント](../cli/usage.md#solana-create-nonce-account)

### 格納されたNonce値のクエリ

耐久性のあるNonceトランザクションを作成するには、保存されているNonce値を、署名および送信時の`--blockhash`引数の値として渡す必要があります。 現在保存されている"nonce値"を取得するには以下のようにします。

- Command

```bash
solana nonce nonce-keypair.json
```

- 出力

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [完全な使用方法ドキュメント](../cli/usage.md#solana-get-nonce)

### 保存されたNonceの値を進める

通常、より有用なトランザクションの外で必要とされるわけではありませんが、格納されたnonce の値は次のようになります。

- Command

```bash
solana new-nonce-keypair.json
```

- 出力

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [完全な使用方法ドキュメント](../cli/usage.md#solana-new-nonce)

### Nonce アカウントを表示

より人間に近い形式でnonceアカウントを検査します。

- Command

```bash
solana nonce-account nonce-keypair.json
```

- 出力

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [完全な使用方法ドキュメント](../cli/usage.md#solana-nonce-account)

### Nonce アカウントから資金を出金する

"nonce アカウント"から資金を出金するには次のようにします。

- Command

```bash
solana draw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- 出力

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> 完全な残高を撤回してNonceアカウントを閉鎖する

> [完全な使用方法ドキュメント](../cli/usage.md#solana-withdraw-from-nonce-account)

### Nonce アカウントに新しい権限を割り当てる

作成後 nonce アカウントの権限を再割り当てるのは次のようにします。

- Command

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json
```

- 出力

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [完全な使用方法ドキュメント](../cli/usage.md#solana-authorize-nonce-account)

## 耐久性のあるNonceをサポートするその他のコマンド

他の CLI サブコマンドとの耐久性のあるnonce を使用するには、2 つの引数がサポートされている必要があります 。

- `--nonce`, nonce 値を格納するアカウントを指定する
- `--nonce-authority`, 省略可能な [nonce authority](#nonce-authority) を指定する

以下のサブコマンドはこれまでにこの扱いを受けています。

- [`支払い`](../cli/usage.md#solana-pay)
- [`ステーキングのデリゲート`](../cli/usage.md#solana-delegate-stake)
- [`ステーキングの無効化`](../cli/usage.md#solana-deactivate-stake)

### 耐久性のあるNonceを使った支払い例

ここでは、"Alice"が"Durable Nonce"を使って"Bob"に"1 SOL"を支払う様子を紹介します。 この手順は耐久性のあるnonceをサポートするすべてのサブコマンドで同じです。

#### アカウントを作りましょう。

最初に、"アリス"、"アリスのnonce"、"ボブ"のアカウントが必要です

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - アリスの口座に入金する

"アリス"は、nonceアカウントを作成し、"ボブ"に送金するためにいくつかの資金が必要になります。 SOLをエアドロップしましょう。

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - アリスのnonceアカウントを作成しよう。

今、アリスはnonceアカウントが必要です。 一つ作成しましょう

> ここでは、個別の [nonce authority](#nonce-authority) が使用されていないため、 `alice.json` は nonce アカウントに対する完全な権限を持ちます。

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - ボブへの支払いに失敗しました

"アリス"は"ボブ"に支払おうとしますが、署名するのに時間がかかりすぎます。 指定されたブロックハッシュの有効期限が切れると、トランザクションが失敗します

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - Nonce助けて！

アリスはトランザクションを再試行し、今度はnonceアカウントとそこに保存されているブロックハッシュを指定します。

> この例では`"alice.json"`が["nonceの権限"](#nonce-authority)を持っていることを覚えておいてください。

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.0013646 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce.json bob.json 1
HR1368UKHVZyenMH7yVz5sBAijV6XAPeWbEXEGVYQorRMcoijeNabzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - 成功！

トランザクションは成功しました！ "ボブ"は"アリス"と"アリスの貯蔵庫"から1つのSOLを受け取ります nonceは新しい値に進みます

```bash
$ solana balance -k bob.json
1 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
