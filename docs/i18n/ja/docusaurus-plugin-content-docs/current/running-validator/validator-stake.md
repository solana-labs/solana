---
title: ステーキング
---

**デフォルトでは、バリデータにはステーキングがありません**。つまり、そのバリデータはリーダになる資格がありません。

## Monitoring Catch Up

ステーキングを委任するには、まずバリデータが起動していて、クラスタに追いついていることを確認します。 バリデータが起動してから追いつくまでには多少時間がかかるかもしれません。 `catchup` コマンドを使用して、このプロセスを通してバリデータを監視します。

```bash
solana catchup ~/validator-keypair.json
```

バリデータが追いつくまでは、正常に投票できず、ステーキングをデリゲートすることはできません。

また、クラスタのスロットが自分よりも速く進んでいることがわかった場合は追いつくことはありません。 これは通常、バリデータとクラスタの残りの部分との間に何らかのネットワーク上の問題があることを意味します。

## ステーキングキーペアを作成

まだ完了していない場合は、ステーキングキーペアを作成してください。 このステップを完了した場合は、ランタイムディレクトリに"validator-stake-keypair.json"が表示されます。

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## ステーキングをデリゲートしよう

最初にステーキングアカウントを作成することで、バリデータに 1 SOLをデリゲートします。

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

そしてそのステーキングをバリデータにデリゲートします。

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> 残りのSOLをデリゲートしないでください。バリデーターはこれらのトークンを投票に使用します

ステーキングは、同じコマンドでいつでも別のノードに再デリゲートできますが、エポックごとに1つの再デリゲートしか許可されません:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

ノードが投票されていると仮定すると、今あなたはバリデータを実行して報酬を生成します。 報酬はエポック境界に自動的に支払われます。

獲得した報酬は、投票アカウントに設定された手数料率に従って、ステーキングアカウントと投票アカウントに分割されます。 報酬はバリデータが起動している間のみ獲得できます。 さらに、一度ステーキングされるとバリデータはネットワークの重要な一部分になります。 ネットワークからバリデータを安全に削除するには、ステーキングを無効にします。

各スロットの最後に、バリデータは投票トランザクションを送信することが期待されます。 これらの投票トランザクションは、バリデータのIDアカウントからランポートによって支払われます。

これは通常のトランザクションで、標準の取引手数料が適用されます。 トランザクション手数料の範囲はジェネシスブロックによって定義されます。 実際の手数料はトランザクション負荷に基づいて変動します。 トランザクションを送信する前に、 [RPC API “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash) で現在の手数料を決定できます。

[トランザクション手数料についてはこちら](../implemented-proposals/transaction-fees.md)をご覧ください。

## 承認ステーキングウォーム アップ

コンセンサスに対する様々な攻撃に対抗するために、新しいステーキングデリゲーションは [ウォーム アップ](/staking/stake-accounts#delegation-warmup-and-cooldown) 期間の対象となります。

ウォームアップ中のバリデータのステーキングのモニター：

- 投票アカウントを表示:`solana vote-account ~/vote-account-keypair.json` これは、バリデータがネットワークに送信したすべての投票の現在の状態を表示します。
- あなたのステーキングアカウント、委任の設定とあなたのステーキングの詳細を表示します:`solana stake-account ~/validator-stake-keypair.json`
- `solana validators` はあなたのものを含むすべてのバリデータの現在の有効なステーキングを表示します
- `Solana Stake-history` は、最近のエポックに対するウォームアップとクールダウンの履歴を示しています
- 次のリーダースロットを示すバリデータのログメッセージを探してください: `[2019-09-27T20:16:00.31972114Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> 投票し、チェックの高さでPoHをリセットします ####. 次のリーダースロットは####`
- ステーキングがウォームアップされると`solana validators`を実行することであなたのバリデータのリストが表示されます。

## ステーキング先のバリデータの監視

バリデータが [リーダ](../terminology.md#leader)になることを確認してください。

- バリデータが追加された後 `solana balance` コマンドを使用して、あなたのバリデータがリーダとして選択され、取引手数料を徴収します。
- Solanaノードは、ネットワークと検証者の参加に関する情報を返す便利な JSON-RPC メソッドを数多く提供します。 JSON-RPCでフォーマットされたデータに必要なメソッドを指定し、curl \(or other http client of your choose\) を使用してリクエストを行います。 例:

```bash
  // Request
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Result
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

役に立つJSON-RPCメソッド:

- `getEpochInfo`[An epoch](../terminology.md#epoch) is the time, i.e. number of [slots](../terminology.md#slot), for which a [leader schedule](../terminology.md#leader-schedule) is valid これは、現在の時代が何であり、クラスタがどの程度まであるかを教えてくれます。
- `getVoteAccounts` これは、バリデータが現在どのくらい有効にステーキングしているかを示します。 バリデータのステーキングの%がエポック境界上で有効になります。 Solana [](../cluster/stake-delegation-and-rewards.md) でステーキングの詳細を学ぶことができます。
- `getLeaderSchedule` 任意の時点で、ネットワークは1つのバリデータだけが台帳エントリを作成することを期待しています。 現在、台帳エントリ [を生成するために選択されている](../cluster/leader-rotation.md#leader-rotation) バリデータは「リーダ」と呼ばれます。 これにより、現在アクティブ化されているステークの完全なリーダスケジュール\(スロットバイスロットベース\) が返されます。 ここでは1つ以上の公開キーが現れます。

## ステーキングを無効にする

クラスタからバリデータをデタッチする前に、以前にデリゲートされたステーキングを無効にする必要があります。

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

ステーキングは即座に無効化されず、ステーキングウォームアップとして同様の方法でクールダウンします。 ステーキングがクールダウンされている間、バリデータはクラスタに取り付けられたままにしてください。 クールダウン中、ステーキングは引き続き報酬を獲得します。 ステーキングのクールダウン後にバリデータをオフにするか、ネットワークから引き出すことが安全です。 クールダウンを完了するには、アクティブなステーキングとステーキングのサイズに応じていくつかのエポックが必要です。

ステーキングアカウントは、無効化後に一度しか使用できないことに注意してください。 cliの `引き出しステーキング` コマンドを使用して、以前にステーキングされたランポートを回収します。
