---
title: Blockstore（区块存储）
---

当一个区块到达最终状态时，从那个区块到创世区块之间所有区块形成了一条链，就是大家都熟悉区块链。 然而，在此之前，验证节点必须维护所有可能有效的链，称为_分叉（forks）_。 领导节点从验证节点中通过算法轮换选举，这个过程会自然的[产生分叉](../cluster/fork-generation.md)。 本节描述了验证节点的 _区块存储（blockstore）_ 数据结构是如何复制并处理这些分叉，直到确定最终的区块。

验证节点记录它在网络上观察到的每一个区块分片（无论任何顺序，只要区块分片是由给定插槽的领导者签名的）。

碎片被移动到一个可分叉的区间`领导者插槽` + `碎片索引` \(在一个 slot 区间\) 。 这允许Solana中的跳表数据结构完整的存储碎片，而无需事先选择跟随哪个分叉或者等待区块已经达到最终状态。

可以从内存中或者最近的文件中重组最新的区块碎片，较晚的区块碎片可以从磁盘的旧文件中重组，这取决于BlockStore的实现。

## Blockstore 功能

1. 持久性：Blockstore和节点的验证流水线打交道

   接收输入流和签名验证。 如果

   收到的区块碎片和领导节点的签名是一致的(

   例如领导节点的分配插槽内接收的区块)，它需要立即存储。

2. 重组：重组和上面提到的一样

   但是它能够为任何接收的区块碎片进行重组。 Blockstore存储带有签名的区块碎片，

   保留区块链的历史记录。

3. 分叉：Blockstore支持区块碎片的随机访问，

   因此可以支持验证节点回滚并且重放记录点后的交易。

4. 重启：通过适当的修剪/剔除，

   区块储藏可以通过少数交易条目回放到 slot 0。 回放的逻辑就是

   ()通过最新的Blockstore状态

   来处理的，例如处理分叉。

## Blockstore 设计

1. 每条记录都是通过键值对来存储的，键就是插槽索引加上区块碎片索引，值就是条目数据。 值得注意的是每个碎片索引都是重新从零开始的(按照每个插槽)。
2. Blockstore 中每个插槽中的元组 `SlotMeta` 数据结构包含：

   - `slot_index` - 插槽索引
   - `num_blocks` - 插槽的区块数量 \(用于区块链中前一个插槽\)
   - `consumed` - 碎片索引的连续最高值 `n`，对于所有的 `m < n`,，总会有一个碎片索引的值等于 `n` \(例如连续最高的碎片索引\)。
   - `received` - 碎片索引的最高值
   - `next slots` - 下一个插槽的列表 可以用于重建到

     一个记录点的回放

   - `last _index` - 插槽的最后一个碎片标识 当领导节点在传输插槽的最后一个碎片索引时会带上这个标识。
   - `is_root` - 如果为True那么当前插槽所有区块碎片都是收集完整的。 我们可以按照以下的规则进行。 假设slot\(n\)是索引为`n`的插槽，并且slot\(n\).is_full\(\)为真，如果索引为`n`的插槽都收集到了所有的碎片。 假设 is_rooted\(n\) 表示“slot\(n\).is_root 为真”。 那么：

     is_rooted\(0\) is_rooted\(n+1\) iff\(is_rooted\(n\) and slot\(n\).is_full\(\)

3. Chaining - 当`x`插槽的碎片到达时，我们检查新插槽中的\(`num_blocks`\)(这个信息在碎片中会有编码)。 我们就知道这个新插槽链是 `x - num_blocks` 插槽。
4. Subscriptions - Blockstore记录一组已“订阅”到的插槽。 这意味着这些插槽的条目将被发送到Blockstore通道，供ReplayStage使用。 详情请查看 `Blockstore APIs`。
5. Update notifications - 对于任意一个 `n`，Blockstore 更新 slot\(n\).is_rooted 这个值从false 变成true。

## Blockstore 接口

Blockstore提供了一个基于订阅的API，ReplayStage使用它来请求所需要的条目。 Blockstore有暴露相关的接口让请求方获取这些条目。 这些订阅API如下所示： 1. `fn get_slots_slots_slouts(slot_indexes: &[u64]) -> Vec<SlotMeta>`：返回`slot_indexes`中对应的区块碎片数据。

1. `fn get_slot_entries(slot_index: u64, entry_start_index: usize, max_entries: Option<u64>) -> Vec<Entry>`：返回以 `entry_start_index` 开头的插槽入口向量，如果 `max_entries == Some(max)`，则将结果限制在 `max`，对返回向量的长度施加限制。

请注意：这意味着重播阶段现在必须知道一个slot何时结束，并订阅它感兴趣的下一个slot以获得下一组条目。 较旧的slot将会存储到Blockstore当中。

## 与Bank交互

Bank 暴露以下字段用于交易重放：

1. `prev_hash`：PoH链上的最后一个区块哈希

   它处理过的条目

2. `tick_head`：PoH链中的滴答，由 Bank

   进行验证

3. `votes`：包含以下内容的一堆记录： 1. `prev_hashes`: 上一次PoH的哈希值 2. `tick_high`：此投票的高度 3. `lockout period`：这个投票的最大截止时间

   能够在这次投票中被引用

在这个voteReplay阶段会建立分叉点，用Blockstore API找到它可以挂起的最长链。 如果当前链没有最新的投票，那么重放阶段将回滚到投票的分叉点，并从那里重新同步。

## Blockstore 剪枝

一旦Blockstore中记录太旧了，表示所有可能的分叉就变得不那么有用了，甚至可能在重启时重放出现问题。 一旦验证节点的投票达到最大截止时间限制，任何不在PoH链上的区块都可以被剪枝、删除。
