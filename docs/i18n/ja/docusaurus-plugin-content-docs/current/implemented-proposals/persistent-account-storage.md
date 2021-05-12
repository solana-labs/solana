---
title: 永続的なアカウントストレージ
---

## 永続的なアカウントストレージ

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. 各バリデータはこのセット全体を管理する必要があります。 Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

"NVME"のようなパーシステントストレージは"DDR"に比べて、20〜40 倍も安価です。 The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## AppendVec

"AppendVec"は、1 つのアペンドオンリーのライターと同時にランダムリードを可能にするデータ構造です。 "AppendVec"の容量を増やしたりサイズを変更したりするには、排他的なアクセスが必要です。 これは、アトミック`オフセット`で実装されており、追加が完了した時点で更新されます。

"AppendVec"の基盤となるメモリは、メモリマップドファイルです。 メモリマップドファイルは高速なランダムアクセスを可能にし、ページングは OS が行います。

## アカウントインデックス

アカウントインデックスは現在フォークされているすべての"アカウント"に対して、単一のインデックスをサポートするように設計されています。

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

インデックスはアカウントの公開キーをフォークのマップにマッピングしたもので、"Account データ"の位置は"AppendVec"になっています。 特定の"Fork"に対する"Account"のバージョンを取得するには以下のようにしてください。

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

読み取りは、格納されたオフセットで`AppendVecId`のメモリマップされた場所を指すことで満たされます。 コピーがなくても参照を返すことができます。

### ルートフォーク

[タワー BFT](tower-bft.md)は最終的にフォークをルートフォークとして選択し、そのフォークはスカッシュされます。 "squashed/root fork"はロールバックできません。

フォークが潰されると、その親の中でフォークにまだ存在していないすべてのアカウントがインデックスを更新することで、フォークに引き上げられます。 潰されたフォークで残高がゼロになったアカウントは、インデックスの更新によってフォークから削除されます。

アカウントは、スカッシュによって到達できなくなったときに、* ガベージコレクション*されることがあります。

3 つの可能なオプションが存在します。

- ルートフォークの"HashSet"を維持します。 秒ごとに 1 つ作成される予定です。 ツリー全体は後でガベージコレクションされます。 あるいは、すべてのフォークがアカウントの参照カウントを保持していれば、インデックスの位置が更新されるたびにガベージコレクションを行うことができます。
- 剪定されたフォークをインデックスから削除します。 残っているフォークのうちルートよりも数が少ないものは、ルートとみなすことができます。
- インデックスをスキャンして、古いルートを新しいものに移行します。 新しいルートよりも下位のフォークが残っていれば、後で削除することができます。

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- アペンドのみの書き込みも高速です。 SSD や NVME、そして OS レベルのカーネルデータ構造のおかげで、PCI や NVMe の帯域幅が許す限り、アペンドを高速に実行することができます \(2,700 MB/s\)。
- "リプレイ"と"バンキング"の各スレッドは、それぞれの"AppendVec"に同時に書き込みを行います。
- 各"AppendVec"は、別の"NVMe"上でホストされる可能性があります。
- 各"リプレイ"と"バンキングのスレッド"は、書き込みをブロックすることなく、すべての"AppendVec"への同時読み取りアクセスが可能です。
- インデックスの書き込みには排他的な書き込みロックが必要です。 "HashMap"の更新におけるシングルスレッドのパフォーマンスは、1 秒あたり 10m 程度です。
- "バンキング"および"リプレイステージ"では、"NVMe"あたり 32 スレッドを使用する必要があります。 "NVMes"は 32 人の同時リーダまたはライタで最適な性能を発揮します。
