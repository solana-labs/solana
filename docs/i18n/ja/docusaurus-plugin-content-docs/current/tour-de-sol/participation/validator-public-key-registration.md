---
title: バリデータ公開キーの作成
---

参加するには、まず登録する必要があります。 [登録情報](../registration/how-to-register.md) を参照してください。

割り当てられたSOLを取得するためには、"keybase.io"アカウントでバリデータの公開キーを発行する必要があります。

## **キーペアの作成**

1. まだ実行していない場合は、バリデータのIDキーペアを生成してください。

   ```bash
     solana-keygen new -o ~/validator-keypair.json
   ```

2. "Id公開キー"は、実行することで確認できるようになりました。

   ```bash
     solana-keygen pubkey ~/validator-keypair.json
   ```

> 注: "validator-keypair.json" ファイルは \(ed25519\) 秘密キーでもあります。

バリデータのIDキーペアは、ネットワーク上であなたのバリデータを一意に識別するものです。 **この情報のバックアップは非常に重要です。**

この情報をバックアップしておかないと、万が一"VALIDATOR"にアクセスできなくなったときに、"VALIDATOR"を復元することができません。 そうなると、割り当てられていたSOLも失われてしまいます。

バリデータを識別するキーペアをバックアップするには、 **"validator-keypair.json" ファイルを安全な場所にバックアップします。**

## Solanaの公開キーを"Keybase"アカウントにリンクしよう

あなたの"Solana 公開キー"を "Keybase.io" アカウントにリンクする必要があります。 以下の説明では、サーバーに"Keybase"をインストールすることで、その方法を説明します。

1. [キーベース](https://keybase.io/download) をマシンにインストールします。
2. サーバの"Keybase"アカウントにログインします。 まだ"Keybase"アカウントを持っていない場合は、まず"Keybase"アカウントを作成します。 基本的なKeybase CLI コマンド [の](https://keybase.io/docs/command_line/basics)リストはこちらです。
3. 公開ファイルフォルダにSolanaディレクトリを作成: `mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. 次の形式でキーベースの公開ファイルフォルダに空のファイルを作成して、バリデータの公開キーを公開します: `/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`. 例:

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. 公開キーが公開されたことを確認するには、 `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`
