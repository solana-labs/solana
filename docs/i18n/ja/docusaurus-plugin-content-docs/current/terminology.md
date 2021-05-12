---
title: 専門用語
---

このドキュメントでは以下の用語が使用されています。

## アカウント

これはアドレスを指定した永続的なファイルで、 [public key](terminology.md#public-key) と [lamports](terminology.md#lamport)がその有効期限を追跡します。

## アプリ

Solana クラスタと対話するフロントエンドアプリケーション。

## バンクステート

与えられた[tick height](terminology.md#tick-height)で台帳上のすべてのプログラムを解釈した結果を指します。 これは少なくともゼロではない[ネイティブトークン](terminology.md#native-tokens)を保持しているすべての[口座](terminology.md#account)のセットを含みます。

## ブロック

[投票](terminology.md#entry) で覆われた台帳の [エントリ](terminology.md#ledger-vote) の連続集合。 [リーダ](terminology.md#leader)は[スロット](terminology.md#slot)ごとに最大でも 1 つのブロックを生成します。

## ブロックハッシュ

指定された [ブロック高さ](terminology.md#hash) の [台帳](terminology.md#ledger) のプリイメージ抵抗性 [ハッシュ](terminology.md#block-height)。 スロットの最後の [エントリ Id](terminology.md#entry-id) から取得しました。

## ブロックの高さ

現在のブロックの下にある [ブロック](terminology.md#block) の数。 [ジェネシスブロック](terminology.md#genesis-block) の後にある最初のブロックは、高さ 1 です。

## ブートストラップバリデータ

[ブロック](terminology.md#validator) を生成する最初の [バリデータ](terminology.md#block)。

## CBC ブロック

元帳内で最も小さく暗号化された塊（セグメント）は、多くの CBC ブロックから成り立っています。 `ledger_segment_size / cbc_block_size` を正確に指定します。

## クライアント

[クラスタ](terminology.md#node) を利用する [ノード](terminology.md#cluster)。

## クラスタ

単一の[台帳](terminology.md#ledger)を管理する[バリデータ](terminology.md#validator)セット

## 確認時間

[リーダ](terminology.md#leader)が[エントリにチェック](terminology.md#tick)を入れてから[確定ブロック](terminology.md#confirmed-block)を作成するまでの持続時間。

## 承認済みブロック

リーダと一致する台帳の解釈の下、[台帳表](terminology.md#ledger-vote)の[過半数超](terminology.md#supermajority)を獲得した[ブロック](terminology.md#block)のこと。

## 制御面

[クラスタ](terminology.md#cluster)の全[ノード](terminology.md#node)を接続するゴシップネットワーク

## クールダウン期間

[ステーキング](terminology.md#epoch) 後の [エポック](terminology.md#stake) の数が無効化され、次第に引き出し可能になります。 この期間中、ステーキングは"deactivating"と見なされます。 詳細情報: [ウォームアップとクールダウン](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)

## クレジット

[vote credit](terminology.md#vote-credit) をご覧ください。

## データ面

[項目](terminology.md#entry) を効率的に検証し、コンセンサスを得るために使用されるマルチキャストネットワーク。

## drone

ユーザの秘密キーの保管者として機能するオフチェーンサービスのこと。 一般的には、トランザクションの検証と著名を行うためのサービスのことを指す。

## エントリ

[台帳](terminology.md#ledger) のエントリ、 [tick](terminology.md#tick) または [transactions entry](terminology.md#transactions-entry) のいずれか。

## エントリー ID

[は、](terminology.md#hash) エントリの最終的な内容に対して、 [エントリの](terminology.md#entry) をグローバルに一意の識別子として機能します。 ハッシュは以下の証拠となります。

- 時間の経過後に生成されるエントリ
- 指定された [トランザクション](terminology.md#transaction) はエントリに含まれるトランザクションです
- [台帳](terminology.md#ledger)の他の項目に対するエントリの位置

[Proof of History](terminology.md#proof-of-history)を参照してください。

## エポック

[リーダスケジュール](terminology.md#slot)が有効な時間、つまり [スロット](terminology.md#leader-schedule) の数。

## フィーアカウント

トランザクションのフィーアカウントは、台帳にトランザクションを含んだコストを支払うためにあります。 これはトランザクションの最初のアカウントです. このアカウントはトランザクションを行うために支払うため、トランザクションが書き込み可能であることを宣言する必要があります(writable)。

## ファイナリティ

[ステーキング](terminology.md#stake) の 2/3 を表すノードに共通の [ルート](terminology.md#root)がある場合。

## フォーク

共通項目から派生した[台帳](terminology.md#ledger)が、その後分岐したもの。

## ジェネシスブロック

チェーンの最初の [ブロック](terminology.md#block)。

## ジェネシスの構成

[genesis ブロック](terminology.md#ledger) の [ledger](terminology.md#genesis-block) の準備をする設定ファイル。

## ハッシュ

バイト列のデジタルフィンガープリント。

## インフレ

トークン供給量の長期的増加は、"検証のための報酬"と"Solana の継続的な開発のための資金"に使用されています。

## 命令

[プログラム](terminology.md#program) の最小単位。 [クライアント](terminology.md#client) は [トランザクション](terminology.md#transaction) に含めることができます。

## キーペア

[公開キー](terminology.md#public-key) と対応する [秘密キー](terminology.md#private-key)。

## ランポート

0.000000001 [sol](terminology.md#native-token) の値を持つフラクショナル [ネイティブトークン](terminology.md#sol)。

## リーダ

[帳面](terminology.md#ledger)に[エントリ](terminology.md#entry)を追加する際の[バリデータ](terminology.md#validator)の役割のこと。

## リーダスケジュール

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). クラスタは、リーダスケジュールを使用して、どのバリデータが [リーダ](terminology.md#leader) であるかを決定します。

## 台帳

[クライアント](terminology.md#entry) によって署名された [トランザクション](terminology.md#transaction) を含む [](terminology.md#client)エントリのリスト。 概念的には、これは [ジェネシスブロック](terminology.md#genesis-block)まで遡ることができます。 しかし、実際の [ バリデータ ](terminology.md#genesis-block) の台帳には、新しい [ブロック](terminology.md#validator) ブロックしか含まれていない可能性があります。

## 台帳投票

指定された [ブロック高さ](terminology.md#hash) の [台帳](terminology.md#bank-state) のプリイメージ抵抗性 [ハッシュ](terminology.md#tick-height)。 これは受け取った[ブロック](terminology.md#block)が検証済みであることを[バリデータ](terminology.md#validator)が確認することと、競合する[ブロック](terminology.md#block)\(つまり[フォーク](terminology.md#fork)\)に投票しないことを[約束すること](terminology.md#lockout)で構成されています。

## ライトクライアント

有効な [クラスタ](terminology.md#client) を指していることを確認できる [クライアント](terminology.md#cluster) の一種。 これは、 [シンクライアント](terminology.md#thin-client) よりも多く、 [バリデータ](terminology.md#validator) より小さい台帳の検証を実行します。

## ローダ

他のオンチェーンプログラムのバイナリエンコーディングを解釈する機能を持つ [プログラム](terminology.md#program)。

## ロックアウト

[バリデータ](terminology.md#validator)が他の[フォーク](terminology.md#fork)時に[投票](terminology.md#ledger-vote)できない期間を指します。

## ネイティブトークン

[トークン](terminology.md#token) は、クラスタ内の [ノード](terminology.md#node) による作業の追跡に使用されます。

## ノード

[クラスタ](terminology.md#cluster) に参加しているコンピュータ 。

## ノード数

[ブロック](terminology.md#validator) を生成する最初の [バリデータ](terminology.md#cluster)。

## PoH

[Proof of History](terminology.md#proof-of-history)を参照してください。

## ポイント

報酬制度における加重 [クレジット](terminology.md#credit)。 [バリデータ](terminology.md#validator)[報酬制度](cluster/stake-delegation-and-rewards.md)では交換時に[ステーキング](terminology.md#stake)に支払われる量は、獲得した[投票クレジット](terminology.md#vote-credit)とステーキングされたランポート数の積になります。

## 秘密キー

[キーペア](terminology.md#keypair)の秘密キー。

## プログラム

[命令](terminology.md#instruction) を解釈するコード。

## プログラム ID

[プログラム](terminology.md#account) を含む [アカウント](terminology.md#program) の公開キー

## PoH

証明の積み重ねで、証明が作成される前にデータが存在し、前の照明の更に前に正確な時間が経過していることを証明するもの。 [VDF](terminology.md#verifiable-delay-function)のように、PoH は作成にかかった時間よりも短い時間で検証することが出来ます。

## 公開キー

[キーペア](terminology.md#keypair)の公開キー。

## ルート

[ブロック](terminology.md#block) または [スロット](terminology.md#slot) で最大 [ロックアウト](terminology.md#lockout) に達した [バリデータ](terminology.md#validator)。 ルートは、バリデータ上のすべてのアクティブフォークの祖先である最も高いブロックです。 ルートはバリデータ上のすべてのアクティブなフォークの祖先である最上位のブロックです。 ルートの祖先ブロックも全てルートになります。先祖でも子孫でもないルートブロックはコンセンサスの検討対象から除外され、破棄される可能性があります。

## ランタイム

[プログラム](terminology.md#validator) の実行を担当する [バリデータのコンポーネント](terminology.md#program)

## シュレッド

[ブロック](terminology.md#block); [バリデータ](terminology.md#validator) の間で送信される最小単位。

## 署名

R (32 バイト) と S (32 バイト) の 64 バイトの ed25519 署名。 R は小さな次数でないエドワーズポイントであり、S は 0<＝ S< L の範囲内であるという要件を持ちます。この要件により、署名の可鍛性が確保されます。 各トランザクションには、 [手数料アカウント](terminology#fee-account)に少なくとも 1 つの署名が必要です。 したがって、トランザクション内の最初の署名は [transacton id](terminology.md#transaction-id) として扱うことができます。

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
