---
title: ステーキングアカウントの管理
---

複数のバリデータにステーキングを委任する場合は、それぞれに別のステーキングアカウントを作成する必要があります シード"0"で最初のステーキングアカウント、"1"で 2 番目のアカウント、"2"で 3 番目のアカウント、というように作成するという規則に従えば、`solana-stake-accounts`ツールは、1 回の呼び出しですべてのアカウントを操作することができます。 全アカウントの残高を集計したり、アカウントを新しいウォレットに移動したり、新しい権限を設定したりするのに使えます。

## 使用法

### ステーキングアカウントを作ろう。

ステーキング権限者の公開キーで、派生したステーキングアカウントを作成し、資金を調達します。

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### アカウントを数えよう。

派生するアカウント数をカウントします。

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### ステーキングアカウント残高の取得をしよう。

派生したステーキングアカウントの残高を集計します。

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### ステーキングアカウントアドレスを取得しよう。

与えられた公開キーから導き出された各ステーキングアカウントのアドレスを列挙します。

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### 新しい権限を設定しよう。

派生したステーキングアカウントごとに新しい権限を設定します。

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --draw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-retake-authority <PUBKEY> \
    --num-accounts <NUMBER> ---fee-payer <KEYPAIR>
```

### ステーキングアカウントの再配置

ステーキングアカウントの再配置:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

各ステーキング口座をアトミックにリベースして承認するには、'move' コマンドを使用します。

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
