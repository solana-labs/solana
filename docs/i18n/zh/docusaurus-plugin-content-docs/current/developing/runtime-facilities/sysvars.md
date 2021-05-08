---
title: Sysvar群集数据
---

Solana通过[`sysvar`](terminology.md#sysvar)帐户向程序公开了各种群集状态数据。 这些帐户填充在[`solana-program`开发工具](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html)中发布的已知地址以及帐户布局中，并在下面概述。

要将sysvar数据包括在程序操作中，请在事务处理的帐户列表中传递sysvar帐户地址。 可以像其他任何帐户一样在您的指令处理器中读取该帐户。 始终以*只读方式*访问sysvars帐户。

## 时钟

Clock sysvar包含有关群集时间的数据，包括当前时间段，时期和估计的Wall-clock Unix时间戳。 它在每个插槽中更新。

- 地址：`SysvarC1ock11111111111111111111111111111111`
- 布局：[时钟](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- 栏位：
  - `slot`：当前的插槽
  - `epoch_start_timestamp`：此epoch中第一个插槽的Unix时间戳。 在纪元的第一个时隙中，此时间戳与`unix_timestamp`(如下所示) 相同。
  - `epoch`：当前纪元
  - `leader_schedule_epoch`：已经为其生成了领导者时间表的最新纪元
  - `unix_timestamp`：此插槽的Unix时间戳。

  每个插槽都有一个基于历史证明的估计持续时间。 但实际上，时隙的流逝可能比此估计更快或更慢。 结果，将基于投票验证程序的oracle输入生成插槽的Unix时间戳。 此时间戳的计算方式为投票提供的时间戳估计的赌注加权中位数，以自纪元开始以来经过的预期时间为界。

  更明确地说：对于每个插槽，每个验证节点提供的最新投票时间戳用于生成当前时隙的时间戳估计(自投票时间戳以来经过的插槽假定为Bank:: ns_per_slot)。 每个时间戳估计都与委派给该投票帐户的股份相关联，以按股份创建时间戳分布。 除非将自`epoch_start_timestamp`以来的经过时间与预期经过时间相差超过25％，否则将中值时间戳记用作`unix_timestamp`。

## Epoch时间表

这时间段表sysvar包含在创世中设置的时间段常量，并允许计算给定时间段中的时隙数，给定时隙的时间段等。(注意：时间段时间表与[`leader时间表不同`](terminology.md#leader-schedule))

- 地址：`SysvarEpochSchedu1e111111111111111111111111`
- 布局：[EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## 费用

Fees sysvar包含当前广告位的费用计算器。 它会根据费用调节器在每个时段进行更新。

- 地址：`SysvarFees111111111111111111111111111111111`
- 布局：[费用](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## 指示

指令sysvar在处理消息时在消息中包含序列化的指令。 这允许程序指令引用同一事务中的其他指令。 阅读有关[指令自省](implemented-proposals/instruction_introspection.md)的更多信息。

- 地址：`` Sysvar1nstructions1111111111111111111111111` ``
- 布局：[指令](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/type.Instructions.html)

## 最近的区块散列值

最近的区块哈希系统变量包含活动的最近区块哈希及其关联的费用计算器。 它在每个插槽中更新。

- 地址：`SysvarRecentB1ockHashes11111111111111111111`
- 布局：[RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## 承租

Rent sysvar包含租金。 目前，该比率是静态的，并且是根据发生率设定的。 通过手动激活功能可以修改租金燃烧百分比。

- 地址：`SysvarRent111111111111111111111111111111111`
- 赞成：[出租](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## 插槽哈希

SlotHashes sysvar包含插槽父库的最新哈希。 它在每个插槽中更新。

- 地址：`SysvarS1otHashes111111111111111111111111111111`
- 布局：[SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## 插槽历史

SlotHistory sysvar包含在最后一个时期出现的插槽的位向量。 它在每个插槽中更新。

- 地址：`SysvarS1otHistory11111111111111111111111111111`
- 布局：[SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## 权益历史

StakeHistory sysvar包含每个时期群集范围内的权益激活和停用的历史记录。 在每个时间段开始时都会对其进行更新。

- 地址：`` SysvarStakeHistory11111111111111111111111111` ``
- 布局：[StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
