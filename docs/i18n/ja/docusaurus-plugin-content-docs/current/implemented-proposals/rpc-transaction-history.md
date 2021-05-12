# 長期RPCトランザクション履歴
RPCには、少なくとも6ヶ月分の取引履歴を提供する必要があります。  現在の履歴は数日単位であり、下流のユーザーにとっては不十分であります。

6ヶ月分の取引データは、バリデータの"rocks"台帳には実用的に保存できないので、外部のデータストアが必要となります。   バリデータの"rocksdb"台帳は引き続き主要なデータソースとして機能し、その後は外部のデータストアにフォールバックします。

影響を受けるRPCエンドポイントは以下の通りです。
* [getFirstAvailableBlock](developing/clients/jsonrpc-api.md#getfirstavailableblock)
* [getConfirmedBlock](developing/clients/jsonrpc-api.md#getconfirmedblock)
* [getConfirmedBlocks](developing/clients/jsonrpc-api.md#getconfirmedblocks)
* [getConfirmedSignaturesForAddress](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress)
* [getConfirmedTransaction](developing/clients/jsonrpc-api.md#getconfirmedtransaction)
* [getSignatureStatuses](developing/clients/jsonrpc-api.md#getsignaturestatuses)

[getBlockTime](developing/clients/jsonrpc-api.md#getblocktime)はサポートされていないことに注意してください。https://github.com/solana-labs/solana/issues/10089 は修正され、 `getBlockTime` は削除できます。

いくつかのシステム設計上の制約：
* 保存・検索するデータの量はすぐにテラバイトに跳ね上がり、しかも不変であること。
* システムはSREにとって可能な限り軽量でなければなりません。  例えば、SREが継続的にノードを監視してリバランスする必要のあるSQLデータベースクラスタは望ましくありません。
* データはリアルタイムで検索可能でなければならず、実行に数分または数時間かかるバッチ処理されたクエリは受け入れられません。
* データを利用するRPCエンドポイントにデータを配置するため、世界中にデータを簡単に複製できること。
* 外部データストアとの連携が容易であり、コミュニティでサポートされている安価なコードライブラリに依存しないこと。

これらの制約に基づき、"Google"の"BigTable製品"をデータストアとして選択しました。

## テーブルスキーマ
"BigTableインスタンス"は、すべてのトランザクションデータを保持するために使用され、迅速な検索のために異なるテーブルに分割されます。

既存のデータに影響を与えることなく、いつでも新しいデータをインスタンスにコピーすることができ、すべてのデータは不変です。  一般的には、現在のエポックが完了した時点で新しいデータがアップロードされると考えられていますが、データダンプの頻度には制限がありません。

古いデータのクリーンアップは、インスタンステーブルのデータ保持ポリシーを適切に設定することで自動的に行われ、そのまま消えてしまいます。  そのため、データを追加する順番が重要になります。  例えば、"エポックN"からのデータの後に"エポックN-1"からのデータを追加すると、古いエポックのデータが新しいデータよりも長くなります。  しかし、クエリの結果に_穴_が開くことはあっても、このような順不同の削除は何の問題もありません。  なお、このクリーンアップ方法により、トランザクションデータを無制限に保存することができますが、その制限はそのための金銭的なコストだけです。

このテーブルレイアウトは、既存のRPCエンドポイントのみに対応しています。  将来的に新しいRPCエンドポイントを導入する際には、スキーマを追加したり、必要なメタデータを構築するためにすべてのトランザクションを繰り返し実行したりする必要があるかもしれません。

## BigTable へのアクセス
"BigTable" には "gRPC" エンドポイントがあり、 ["tonic"](https://crates.io/crates/crate) や "raw protobuf API" を使用してアクセスすることができますが、 現在のところ "BigTable" 用の高レベルな Rust クレートは存在しません。  実用的には、"BigTable" のクエリの結果の解析がより複雑になりますが、重要な問題ではありません。

## データ数
インスタンスデータの継続的な収集は、新しい`solana-ledger-tool`コマンドを使用して、指定されたスロット範囲のrockdbデータをインスタンススキーマに変換することにより、エポックサイクルで行われます。

同じプロセスを手動で1回実行し、既存の台帳データを埋め戻します。

### Block Table: `block`

このテーブルには、指定されたスロットの圧縮ブロックデータが含まれています。

行キーは、スロットの16桁の小文字の16進数で生成され、ブロックが確認された最も古いスロットが、行のリストで常に最初になるようにします。  例：スロット42の行キーは、0000000000002aです。

行データは、圧縮された` StoredConfirmedBlock` 構造体です。


### Account Address Transaction Signature Lookup Table: `tx-by-addr`

このテーブルには、指定されたアドレスに影響を与えるトランザクションが含まれています。

行キーは `<base58
address>/<slot-id-one's-compliment-hex-slot-0-prefixed-to-16-digits>` です。  行データは `TransactionByAddrInfo` 構造体を圧縮したものです。

スロットの1の補数を取ることでスロットのリスト化が可能になり、アドレスに影響を与えるトランザクションを持つ最新のスロットが常に最初に表示されるようになります。

"Sysvar"のアドレスにはインデックスが付いていません。  しかし、"Vote" や "System" などの頻繁に使用されるプログラムにはインデックスが付けられており、確認されたスロットごとに行が用意されています。

### Transaction Signature Lookup Table: `tx`

このテーブルは、トランザクション署名、確認されたブロックと、そのブロック内のインデックスにマッピングします。

行のキーはbase58にエンコードされたトランザクション署名です。 行データは、圧縮された`TransactionInfo`構造体です。
