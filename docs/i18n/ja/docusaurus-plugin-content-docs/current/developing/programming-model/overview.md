---
title: "概要"
---

[アプリ](terminology.md#app) は、1つ以上の[トランザクション](transactions.md) を[手順](transactions.md#instructions) で送信することで、Solana クラスターと相互作用します。 Solanaの[ランタイム](runtime.md)は、アプリ開発者がデプロイした[プログラム](terminology.md#program)に、あらかじめそれらの命令を渡しておきます。 命令とは、例えば、ある["ランポート"](terminology.md#lamports)をある["アカウント"](accounts.md)から別の"アカウント"に転送することをプログラムに指示したり、ランポートの転送方法を規定するインタラクティブな契約を作成したりすることです。 命令は、各トランザクションに対して順次およびアトミックに実行されます。 命令が無効な場合、トランザクション内のすべての口座変更は破棄されます。

すぐに開発を開始するには、[例](developing/deployed-programs/examples.md) のいずれかを"ビルド"、"デプロイ"、および"実行"できます。