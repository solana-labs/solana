---
title: バリデータ情報の公開
---

自分のバリデータ情報をチェーンに公開して、他のユーザーから見えるようにすることができます。

## Solana バリデータ情報を実行

"solana CLI"を実行して、バリデータ情報のアカウントを入力します。

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

For details about optional fields for VALIDATOR_INFO_ARGS:

```bash
solana validator-info publish --help
```

## コマンドの例

"publish" コマンドの例:

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

クエリコマンドの例:

```bash
solana validator-info get
```

出力は

```text
8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFIizayqziDc9ah
  バリデータ パブキー: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTtyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https:/elvis-validates.com"}
```

## キーベース

Keybase のユーザー名を含めることで、クライアントアプリケーション\(Solana Network Explorer\など) は自動的にバリデータのパブリックプロファイルを取り込むことができます。 バリデータの公開キーを"Keybase"に接続するには以下のように実行してください。

1. [https://keybase.io/ ](https://keybase.io/)に参加して、バリデータのプロフィールを完成させてください。
2. バリデータの**個別公開キー**を"Keybase"に追加します。

   - ローカルコンピュータに `validator-<PUBKEY>`という空のファイルを作成します。
   - Keybase の"Files"セクションに移動し、公開キーファイルを以下にアップロードします。

     a `solana` subdirectory in your public folder: `/keybase/public/<KEYBASE_USERNAME>/solana`

   - 確認するには、以下の場所を正常にブラウズできることを確認してください。

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. `solana validator-info`に"Keybase"のユーザー名を追加・更新します。 The

   `validator-<PUBKEY>`ファイルの検証を行います。
