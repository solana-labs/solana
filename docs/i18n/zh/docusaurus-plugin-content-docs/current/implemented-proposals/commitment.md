---
title: 承诺
---

承诺度量旨在为客户提供一个衡量特定区块上的网络确认和利益水平的标准。 然后，客户可以使用这些信息来推导出自己的承诺度量。

# 计算远程调用

客户端可以通过`get_block_commitment(s: Signature) -> BlockCommitment`，使用远程调用向验证节点请求签名`s`的承诺指标。 `BlockCommitment`结构包含一个u64`[u64，MAX_CONFIRMATIONS]`的数组。 这个数组表示验证节点投票的最后一个区块`M`时，包含签名`s`的特定区块`N`的承诺度量。

`BlockCommitment`数组中索引`i`处的条目`s`意味着验证节点观察到`s`在某一区块`M`中观察到的集群中的总质押达到`i`个确认的区块`N`。 这个数组中会有`MAX_CONFIRMATIONS`元素，代表从1到`MAX_CONFIRMATIONS`的所有可能的确认数。

# 承诺度量的计算

建立这个`BlockCommitment`结构利用了为建立共识而进行的计算。 `consensus.rs`中的`collect_vote_lockouts`函数建立了一个HashMap，其中每个条目的形式是`(b, s)`，其中`s`是银行`b`的质押数量。

对可投票候选银行`b`的计算如下。

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

其中`f`是一些累积函数，它用一些可从投票`v`和`vote_account`派生的数据(stake、lockout等) 来修改插槽`a`的`stake`条目。 这里注意，这里的`ancestors`只包括当前状态缓存中存在的插槽。 比状态缓存中存在的更早的银行的签名无论如何也查询不到，所以这里的承诺计算中不包括这些银行。

现在，我们自然可以通过以下方法来增强上述计算，为每一个银行`b`也建立一个`BlockCommitment`数组。

1. 增加一个`ForkCommitmentCache`来收集`BlockCommitment`结构
2. 用`f`代替`f'`，使上述计算也为每一个银行`b`建立这个`BlockCommitment`。

由于1) 是不是很重要，所以我们将继续讨论2) 的细节。

在继续之前，值得注意的是，对于某个验证节点的投票账户`a`，该验证节点在插槽`s`上的本地确认数为`v.num_confirmations`，其中`v`是投票堆栈`a.vails`中最小的一票，这样`v.slot >= s`(即不需要查看任何大于v的投票>，因为确认数会更低)。

现在更具体的说，我们把上面的计算增强为。

```text
   let output: HashMap<b, Stake> = HashMap::new();
   let fork_commitment_cache = ForkCommitmentCache::default();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), *fork_commitment_cache.get_mut(a), vote_account, v);
           }
       }
   }
```

其中`f'`被定义为：

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_account: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
    }
```
