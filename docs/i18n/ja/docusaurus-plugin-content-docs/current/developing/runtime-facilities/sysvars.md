---
title: Sysvar Cluster Data
---

Solana は[`sysvar`](terminology.md#sysvar) アカウントを介してさまざまなクラスタ状態データをプログラムに公開します。 これらのアカウントは、[`solana-program` crate](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html)でアカウントのレイアウトとともに公開された既知のアドレスに登録され、以下のように説明されます。

プログラム操作に sysvar データを含めるには、トランザクション内のアカウントのリストに sysvar アカウントアドレスを渡します。 このアカウントは他のアカウントと同様に命令プロセッサで読むことができます。 Access to sysvars accounts is always _readonly_.

## クロック

Clock sysvar は、現在のスロット、エポック、および推定ウォールクロック Unix タイムスタンプを含むクラスタ時刻に関するデータを含みます。 スロットごとに更新されます。

- 住所: `SysvarC1ock1111111111111111111111`
- レイアウト: [時計](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- フィールド:

  - `スロット`: 現在のスロット
  - `epoch_start_timestamp`: この時の最初のスロットの Unix タイムスタンプ。 エポックの最初のスロットでは、このタイムスタンプは `unix_timestamp` (以下) と同じです。
  - `エポック`: 現在のエポック
  - `leader_schedule_epoch`: リーダースケジュールがすでに生成されている最新のエポック
  - `unix_timestamp`: このスロットの Unix タイムスタンプ。

  各スロットには、歴史証明に基づいて推定期間があります。 しかし、実際には、スロットはこの推定よりも速く遅くなる可能性があります。 結果として、スロットの Unix タイムスタンプは、投票バリデーターからのオラクル入力に基づいて生成されます。 このタイムスタンプは、投票で得られたタイムスタンプ推定値のステークス加重中央値として計算され、エポックの開始からの予想経過時間で制限されます。

  より明示的に: 各スロットによって提供された最新の投票タイムスタンプは、現在のスロットのタイムスタンプ推定値を生成するために使用されます。 (投票タイムスタンプは Bank::ns_per_slot) であると仮定されています。 各タイムスタンプの見積もりによってタイムスタンプの分布を作成するために、その投票口座に委任されたステークに関連付けられています。 `epoch_start_timestamp`からの経過時間が予想される経過時間から 25％以上乖離していない限り、中央値のタイムスタンプが`unix_timestamp`として使用されます。

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- アドレス: `SysvarEpochSchedu1e11111111111111`
- レイアウト: [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## 手数料

Fees sysvar には、現在のスロットの料金計算機が含まれます。 料金のガバナーに基づいて、スロットごとに更新されます。

- アドレス: `SysvarFees111111111111111111111111111111111`
- レイアウト: [Fees](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## 命令

Instructions sysvar は Message が処理されている間、Message 内のシリアル化された命令を格納します。 これにより、プログラムの命令が同じトランザクション内の他の命令を参照することができます。 [命令のイントロスペクション](implemented-proposals/instruction_introspection.md)についての詳しい情報はこちらを参照してください。

- アドレス: `Sysvar1nstructions1111111111111111111111111`
- レイアウト: [Instructions](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## 最近のブロックハッシュ

RecentBlockhashes システムバーには、最近のアクティブなブロックハッシュとそれに関連する料金計算機が含まれています。 スロットごとに更新されます。

- アドレス: `SysvarRecentB1ockHashes11111111111111111111`
- レイアウト: [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## レンタル

Rent sysvar には、レンタル料が含まれています。 現在、このレートはジェネシスで設定される静的なものです。 Rent の燃焼率は、手動で機能を起動することで変更されます。

- アドレス: `SysvarRent111111111111111111111111111111111`
- レイアウト: [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## スロットハッシュ

SlotHashes sysvar は、スロットの親バンクの最新のハッシュを含んでいます これはスロットごとに更新されます。

- アドレス: `SysvarS1otHashes111111111111111111111111111`
- レイアウト: [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## スロットヒストリ

SlotHistory sysvar は最後のエポックに存在するスロットのビットベクトルを含みます スロットごとに更新されます。

- アドレス: `SysvarS1otHistory11111111111111111111111111`
- レイアウト: [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## ステーキングヒストリ

StakeHistory はエポックごとのクラスタ全体のステークの活性化と非活性化の履歴を格納しています。 これは各エポックの開始時に更新されます。

- アドレス: `SysvarStakeHistory1111111111111111111111111`
- レイアウト: [StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
