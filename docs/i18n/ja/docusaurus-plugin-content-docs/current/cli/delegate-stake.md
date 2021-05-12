---
title: ステーキングをデリゲートしよう。
---

[SOL](transfer-tokens.md)を受け取った後は、バリデータに *ステーキング*を委ねて活用することも考えられます。 ステーキングとは、 *ステーキングアカウント*にあるトークンのことです。 Solana はバリデーターの投票を、彼らに委任されたステーキングの量によって重み付けし、ブロックチェーンの次の有効な取引ブロックを決定する際に、バリデーターがより大きな影響力を持つようにします。 Solana はその後、定期的に新しい SOL を生成し、ステーカーやバリデーターに報酬を与えます。 委任したステーキング量が多ければ多いほど、より多くの報酬を得ることができます。

## ステーキングアカウントを作ろう。

ステーキングを委任するには、トークンをステークアカウントに移す必要があります。 アカウントを作成するには、キーペアが必要です。 その公開鍵は、 [ステーキングアカウントアドレス](../staking/stake-accounts.md#account-address)として使用されます パスワードや暗号化は必要ありません。このキーペアは、ステーキングアカウントの作成後すぐに破棄されます。

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

出力は、テキスト `pubkey:` の後に公開キーを含みます。

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

公開キーをコピーして保管してください。 次に作成するステーキングアカウントに対してアクションを実行する際に必要となります。

さぁ、ステーキングアカウントを作成しましょう。

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<"AMOUNT">` トークンは、"from"の `<キーペア>` から、"stake-account.json の公開キー"の新しいステーキングアカウントに転送されます。

"stake-account.json"ファイルを破棄できるようになりました。 追加のアクションを認可するには、 `"stake-account.json"`ではなく、`"--stake-authority"` または"--withdraw-authority"キーペアを使用します。

`solana stake-account` コマンドを使用して、新しいステーキングアカウントを表示します。

```bash
solana-stake-accounts count <STAKE_ACCOUNT_ADDRESS>
```

出力は以下のようになります:

```text
Total Stake: 5000 SOL
Stake account is unelegated
Stake Authority: EXU95vqs93yPeCeAU7mPpu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPp6HbRumTFPEiGug9oCdvQ5F
```

### ステーキングと引き出し権限を設定しよう。

["ステーキングおよび引き出しの権限"](../staking/stake-accounts.md#understanding-account-authorities)は、アカウント作成時に`"--stake-authority"`および`"--withdraw-authority"`オプションで設定することができ、またアカウント作成後には`"solana stake-authorize"` コマンドで設定することもできます。 例えば、新しいステーキング権限を設定するには、"run:"と入力します。

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

これは、既存のステーキング権限 `<KEYPAIR>` を使用して、ステーキングアカウント `<STAKE_ACCOUNT_ADDRESS>`上の新しいステーキング権限 `<PUBKEY>` を使用して、ステーキングアカウントに新しいステーキング権限を認証します。

### 詳細: Derive Stake Account Addresses

ステーキングを委任すると、ステーキングアカウント内のすべてのトークンを 1 人のバリデータに委任することになります。 複数のバリデータに委任するには、複数のステーキングアカウントが必要となります。 アカウントごとに新しいキーペアを作成し、それらのアドレスを管理するのは面倒なことです。 幸いなことに、`--seed`オプションを使えば ステーキングアドレスを導き出すことができます。

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>`は 32 バイトまでの任意の文字列ですが、一般的にはどの派生アカウントかに対応する数字を指定します。 最初のアカウントは"0"、次に"1"、というようになります。 `<STAKE_ACCOUNT_KEYPAIR>` の公開キーは、をベースアドレスとして動作します。 このコマンドは、ベースアドレスとシード文字列から新しいアドレスを派生させます。 コマンドが派生するステーキングアドレスを確認するには、`solana create-address-with-seed`を使用します。

```bash
solana create-address-with-seed -from <PUBKEY> <SEED_STRING> SAKE
```

`<PUBKEY>` は `<STAKE_ACCOUNT_KEYPAIR>` に渡された `solana create-stake-account` の公開キーです。

このコマンドは、派生アドレスに出力し、ステーキング操作の`<STAKE_ACCOUNT_ADDRESS> `引数に使用することができます。

## ステーキングをデリゲート（委任）しよう。

自分のステーキングをバリデータに委任するには、そのバリデータの投票アカウントのアドレスが必要です このアドレスは、`solana validators`コマンドでクラスタに全バリデータとその投票アカウントのリストを問い合わせることで得られます。

```bash
solana validators
```

各行の 1 列目にはバリデータの ID、2 列目には投票用アカウントのアドレスが入っています。 バリデータを選択し、その投票アカウントアドレスを`solana delegate-stake`で使用します。

```bash
solana delegate-stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

ステーキング権限者`<KEYPAIR>`は、アドレス`<STAKE_ACCOUNT_ADDRESS>`のアカウントに対する操作を許可します ステーキングは、アドレス`<VOTE_ACCOUNT_ADDRESS>`の投票アカウントに委任されています。

ステーキングをデリゲート（委任）した後、`solana stake-account`を使用してステーキングアカウントへの変更を観察します。

```bash
solana-stake-accounts count <STAKE_ACCOUNT_ADDRESS>
```

その際、"Delegated Stake"と"Delegated Vote Account Address"という新しいフィールドが表示されます。 出力は以下のようになります。

```text
Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## ステーキングの無効化

一度委任されたステーキングは、`solana deactivate-stake`コマンドで解除することができます。

```bash
solana deactivate-stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

ステーキング権限者`<KEYPAIR>`が、アドレス`<STAKE_ACCOUNT_ADDRESS>`のアカウントに対する操作を許可します。

ステーキングは "クールダウン "に数エポックかかることに注意してください。 クールダウン期間中にステーキングをデリゲーション（委任）しようとすると失敗します。

## ステーキングを引き出そう。

`solana withdraw-stake`コマンドで、ステーキングアカウントからトークンを送金します。

```bash
solana withdraw-stake --draw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>`は既存のステーキングアカウント、`<KEYPAIR>`は出金権限、`<AMOUNT>`は`<RECIPIENT_ADDRESS>`に送金するトークンの数です。

## ステーキングの分割

既存のステーキングが引き出しの対象とならない間に、追加のバリデータにステーキングを委任したい場合があります これは、現在ステーキングしている、クールダウンしている、ロックされているなどの理由で対象外となる場合があります。 既存のステーキングアカウントから新しいアカウントにトークンを移すには、`solana split-stake`コマンドを使用します。

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>`は既存のステーキングアカウント、`<KEYPAIR>`はステーキング権限、 `<NEW_STAKE_ACCOUNT_KEYPAIR>`は新しいアカウントのキーペアです。 `<AMOUNT>`は新しいアカウントに移行するトークンの数です。

ステーキングアカウントを派生アカウントアドレスに分割するには、`--seed`オプションを使用します。 詳細は["Stake Account Addresses"](#advanced-derive-stake-account-addresses)を参照してください。
