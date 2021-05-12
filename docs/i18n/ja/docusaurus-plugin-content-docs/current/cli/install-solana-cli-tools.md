---
title: Solana Tool Suiteをインストールしよう。
---

ご希望のワークフローに応じて、Solana ツールをコンピュータにインストールする方法は複数あります。

- [Solana のインストールツールを使用する(最も簡単なオプション)](#use-solanas-install-tool)
- [ビルド済みのバイナリをダウンロードしてください。](#download-prebuilt-binaries)
- [ソースからビルド](#build-from-source)

## Solana のインストールツールを使用

### MacOS & Linux

- お気に入りのターミナルアプリケーションを開いてください。

- Solana のリリース[](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION)をマシンにインストールしてください。

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- `LATEST_SOLANA_RELEASE_VERSION`の代わりに、ご希望のソフトウェアバージョンに対応したリリースタグを使用することもできますし、`安定版`、`ベータ版`、`エッジ版`という 3 つのシンボリックなチャンネル名を使用することもできます。

- 以下の出力は更新が成功したことを示します:

```text
downloading LATEST_SOLANA_RELEASE_VERSION installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: LATEST_SOLANA_RELEASE_VERSION
* Release URL: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

- お使いのシステムによっては、インストーラーメッセージの最後に次のようなプロンプトが表示されます。

```bash
PATH環境変数を更新して、solanaのプログラムを含むようにしてください。
```

- 上記のメッセージが表示された場合は、その下の推奨コマンドをコピー＆ペーストして`PATH`を更新してください。
- 目的のバージョンの`solana`がインストールされていることを確認してください。

```bash
solana --version
```

- インストールが成功した後、`solana-install update`を使用すると、いつでも簡単に Solana ソフトウェアを新しいバージョンにアップデートすることができます。

---

### Windows

- 管理者としてコマンドプロンプト(`cmd.exe`)を開きます。

  - Search for Command Prompt in the Windows search bar. コマンドプロンプトが表示されたら、右クリックして"Open as Administrator"を選択します。 "このアプリがあなたのデバイスに変更を加えることを許可しますか?"というポップアップウィンドウが表示されたら、"Yes"をクリックします。

- 以下のコマンドをコピー＆ペーストし、Enter キーを押すと、Solana のインストーラーが一時ディレクトリにダウンロードされます。

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe -create-dirs
```

- 以下のコマンドをコピー＆ペーストして Enter キーを押すと、最新版の Solana がインストールされます。 お使いのシステムでセキュリティのポップアップが表示された場合は、プログラムの実行を許可するように選択してください。

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSI
```

- インストーラが終了したら、Enter を押します。

- コマンドプロンプトウィンドウを閉じて、通常のユーザーとして新しいコマンドプロンプトウィンドウを開き直します。
  - 検索バーで"Command Prompt"と検索し、"Command Prompt"アプリのアイコンを左クリックします（管理者として実行する必要はありません）。)
- 目的のバージョンの`solana`がインストールされていることを確認してください。

```bash
solana --version
```

- インストールが成功した後、`solana-install update`を使用すると、いつでも簡単に Solana ソフトウェアを新しいバージョンにアップデートすることができます。

## ビルド済みのバイナリをダウンロードしてください。

`solana-install`でインストールを管理したくない場合は、手動でバイナリーをダウンロードしてインストールすることができます。

### Linux

[https://github.com/solana-labs/solana/releases/latest ](https://github.com/solana-labs/solana/releases/latest)にアクセスしてバイナリをダウンロードし、**solana-release-x86_64-unknown-linux-msvc.tar.bz2** をダウンロードし、アーカイブを解凍します。

```bash
tarのjxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)に移動してバイナリをダウンロードし、**solana-release-x86_64-apple-darwin.tar.bz2**をダウンロードして、アーカイブを解凍します。

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)に移動してバイナリをダウンロードし、**solana-release-x86_64-pc-windows-msvc.tar.bz2**をダウンロードし、WinZip などを使ってアーカイブを展開してください。

- コマンドプロンプトを開き、バイナリを解凍したディレクトリに移動して実行します。

```bash
cd Solana-release/
set PATH=%cd%/bin;%PATH%
```

## ソースからビルド

ビルド済みのバイナリを使用できない場合や、ソースから自分でビルドしたい場合は、[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)、**ソースコード**のアーカイブをダウンロードしてください。 コードを抽出し、バイナリをビルドします

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

次のコマンドを実行すると、ビルド済みのバイナリと同じ結果が得られます。

```bash
solana-install init
```
