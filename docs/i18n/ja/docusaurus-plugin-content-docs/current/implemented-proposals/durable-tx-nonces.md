---
title: 耐久性のあるトランザクションNonces
---

## 問題

リプレイを防ぐため、Solana トランザクションには"recent" ブロックハッシュ値が入力された nonce フィールドが含まれます。 古すぎるブロックハッシュを含むトランザクション (この記事を書いている時点で〜2 分) は、ネットワークによって無効として拒否されます。 残念ながら、カストディサービスなどの特定のユースケースでは、トランザクションのための署名を作成するために、より多くの時間を必要とします。 これらのオフラインネットワーク参加者を可能にするためにはメカニズムが必要です。

## 必須項目

1. トランザクションの署名は nonce 値をカバーする必要があります
2. Nonce は、キーの開示に署名する場合でも、再利用できません。

## コントラクトベースのソリューション

ここでは、契約ベースの問題解決方法について説明します。 これにより、クライアントはトランザクションの `recent_blockhash`フィールドで将来使用するための nonce 値を "stash" することができます。 このアプローチは、いくつかの CPUISA によって実装された比較とアトミックスワップ命令 に似ています。

耐久性のある nonce を使用する場合、クライアントは最初にアカウントデータからその値をクエリする必要があります。 トランザクションは通常の方法で構築されますが、以下の追加の要件で構築されます:

1. 耐久性のある nonce 値は `recent_blockhash` フィールドで使用されます。
2. `AdvanceNonceAccount` 命令はトランザクションで最初に発行されます。

### コントラクトメカニズム

TODO: svgbob これをフローチャートへ

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

この機能を使用したいクライアントは、システムプログラムの下に nonce アカウントを作成することから始まります。 このアカウントは `初期化されていない` 状態で保存されたハッシュがないため、使用できなくなります。

新しく作成されたアカウントを初期化するには、 `InitializeNonceAccount` 命令を 発行する必要があります。 This instruction takes one parameter, the `Pubkey` of the account's [authority](../offline-signing/durable-nonce.md#nonce-authority). Nonce アカウントは、この機能のデータ永続性の要件を満たすためには、[家賃免除](rent.md#two-tiered-rent-regime)でなければならず、そのため、初期化する前に十分なランポートを預ける必要があります。 初期化が成功すると、クラスターの最新のブロックハッシュが指定された nonce 権限 `公開キー` とともに格納されます。

`AdvanceNonceAccount` 命令は、口座の nonce 値を管理するために使用されます。 アカウントの状態データにクラスターの最新のブロックハッシュが格納されています すでに格納されている値に一致した場合、クラスターのブロックハッシュが失敗します。 このチェックをオンにすると、同じブロック内でトランザクションを再実行できなくなります。

Nonce アカウントには[家賃免除](rent.md#two-tiered-rent-regime)の要件があるため、アカウントから資金を移動させるには、カスタム出金命令が使用されます。 `WithdrawNonceAccount`命令は、引き出したいランポートという 1 つの引数を取り、アカウントの残高が家賃免除の最低額を下回るのを防ぐことで、家賃免除を実施します このチェックの例外は、最終的な残高が 0 lamports になる場合で、この場合、アカウントは削除の対象となります。 このアカウント閉鎖の詳細には、`AdvanceNonceAccount`のように、保存された nonce 値がクラスタの最新のブロックハッシュと一致してはならないという追加要件があります。

アカウントの[nonce 権限](../offline-signing/durable-nonce.md#nonce-authority)は、`AuthorizeNonceAccount`命令を使用して変更できます。 これは、新しい権限の`公開キー`を 1 つのパラメータとして受け取ります。 この命令を実行すると、アカウントと新しい権限のアカウントを完全に制御できます。

> `AdvanceNonceAccount`、`WithdrawNonceAccount`、`AuthorizeNonceAccount`のいずれも、トランザクションに署名するためにアカウントの現在の[nonce 権限](../offline-signing/durable-nonce.md#nonce-authority)を必要とします。

### ランタイムサポート

この機能を実装するにはコントラクトだけでは不十分です。 トランザクションに存在する `recent_blockhash` を強制し、失敗したトランザクションリプレイによる手数料の盗難を防ぐには、実行時の変更が必要です。

通常の `check_hash_age` バリデーションに失敗したトランザクションは、耐久性のあるトランザクションノンスについてテストされます。 これはトランザクションの最初の命令として `AdvanceNonceAccount`の命令を含めることによって通知されます。

ランタイムが耐久性のあるトランザクション Nonce が使用中であると判断した場合、トランザクションを検証するために次の追加アクションを実行します：

1. `Nonce`命令で指定された`NonceAccount`をロードします。
2. `NonceState` は `NonceAccount`のデータフィールドからデシリアライズされ、 `初期化された` 状態にあることが確認されました。
3. `NonceAccount`に格納されている nonce の値が、トランザクションの`recent_blockhash`フィールドで指定されたものと一致するかどうかをテストします。

上記の 3 つのチェックがすべて成功した場合、そのトランザクションは検証を続けることができます。

`InstructionError`で失敗したトランザクションには手数料が課金され、その状態の変更はロールバックされるので、`AdvanceNonceAccount`命令が元に戻された場合、手数料を窃取される可能性があります。 悪意のあるバリデータは、保存されている nonce の処理が完了するまで、失敗したトランザクションを再生することができます。 ランタイムの変更により、この動作を防ぐことができます。 `AdvanceNonceAccount`命令以外の`InstructionError`で durable nonce トランザクションが失敗すると、nonce アカウントは通常通り実行前の状態にロールバックされます。 その後、ランタイムはその nonce 値を前進させ、成功したかのように格納されたアドバンスド nonce アカウントを前進させます。
