## クラスタを再起動しています

### ステップ 1. クラスタが再起動するスロットを特定します。

最も楽観的に確認されたスロットは、開始する最良のスロットは、[この](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71)メトリクスの データポイント を探して見つけることができます。 そうでなければ最後のルートを使用します。

このスロットを `SLOT_X`と呼んでください

### ステップ 2. バリデーター(s) を停止

### ステップ 3. 必要に応じて新しい Solana バージョンをインストールします。

### ステップ 4. スロット `SLOT_X` の新しいスナップショットを作成します。 `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

台帳ディレクトリに新しいスナップショットが含まれるようになりました。 `solana-ledger-tool create-snapshot` will also output the new shred version, and bank hash value, call this NEW_SHRED_VERSION and NEW_BANK_HASH respectively.

バリデータの引数を調整してください：

```bash
 --wait-for-supermaxal SLOT_X
 --expected-bank-hash NEW_BANK_HASHH
```

その後、バリデータを再起動します。

バリデータが起動し、 `SLOT_X`で保持パターンになっているログで確認し、超大多数を待っています。

### ステップ 5. Discord で再起動をアナウンス:

以下のようなものを#アナウンスに投稿します (適切にテキストを調整します):

> Hi @Validators,
>
> "v1.1.12"をリリースし、テストネットを再開する準備が整いました。
>
> Steps:
>
> 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12
> 2. 推奨される方法は、 あなたのローカル台帳から以下を開始します:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. バリデータがスロット最大SLOT_Xまでの台帳を持っていない場合、または台帳を削除した場合は、以下のスナップショットをダウンロードしてください:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     自分の台帳がどのスロットを持っているかは、以下の`solana-ledger-tool -l path/to/ledger bounds` で確認できます。

3. ステーキングの 80％がオンラインになるまで待ちます。

再起動したバリデータが正しく 80%を待っていることを確認するには: a. "gossip"のログメッセージに表示されているアクティブな`賭け金のN%`を確認します。 b. RPC でどのスロットにいるかを尋ねます: `solana --url http://127.0.0.1:8899 slot`. ステーキングが 80%になるまで`SLOT_X`を返すはずです。

ありがとう！

### ステップ 7. 待って聞いてください

再起動時にバリデータを監視します。 質問に答えて、助けてください。
