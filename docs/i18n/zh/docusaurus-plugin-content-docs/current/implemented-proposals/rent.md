---
title: 出租
---

Solana 上的帐户可能处于所有者控制的状态 \(`Account::data`\) 该帐户独立于帐户的余额 \(`Account::lamports`\)。 因为网络上的验证节点需要在内存中保留此状态的工作副本，因此网络需要收取基于时间空间的一部分费用，也称为租金。

## 二级租金制度

那些账户最低余额相当于 2 年租金的账户可以免税。 _2 年_ 是因为硬件成本每 2 年会下降一半的事实，以及由于几何序列引起的收敛。 余额低于此阈值的账户按起始规定的费率收取租金，按每个年份的灯塔收取。 网络基于每个轮次收取租金，同时计入下一个轮次的租金，`Account:::rent_epoch` 也会保存下次应该从帐户中收集的租金。

目前，租金费用从一开始就已经固定了。 然而，我们预计它是动态的，反映了当下的硬件储存成本。 因此，随着技术进步以及硬件成本的下降，价格一般都会逐渐降低。

## 收取租金的时间

从帐户收取租金的时间有 2 次：\(1\) 进行交易，\(2\) 每个轮次定期收一次。 \(1\) 包括创建新账户本身的交易，而且它作为银行加载阶段，进行正常交易期间的一个步骤，。 \(2\) 该步骤是为了确保从旧账户中收取租金，这些账户在最新的轮次中基本都没有被引用。 \(2\) 需要扫描整个帐户，并以帐户地址前缀为基础将其分布到一个轮次，以避免由于收取租金而导致的加载波动。

相反，收取租金不适用于任何协议级账簿程序直接操纵的账户，其中包括：

- 收租本身的分配(以往，这可能导致重复收租的问题)
- 在每个新轮次开始的时候，质押奖励的分配 (尽量减少在新轮次开始时的波动幅度)
- 每个 slot 结束时的交易费分配

即使这些过程超出了收取租金的范围，所有被操纵的账户最终都将由 \(2\) 机制处理。

## 收集租金的实际过程

租金到期的期限为一个 epoch，取决于租金制度，帐户有 `current_epoch` 或 `current_epoch + 1` 的 `Account::rent_epoch` 。

如果帐户处于免责状态， `Account::rent_epoch` 就只更新到 `current_epoch`。

如果帐户不处于免责状态，下一个 epoch 和 `Account::rent_epoch` 之间的差额用于计算此账户所欠的租金金额\(通过 `Rent::due()`\)。 计算中的任何分数端口灯都是被截断的。 已从 `Account::lamport` 和 `Account::rent_epoch` 更新到 `current_epoch + 1` (= 下一个 epoch)。 如果到期租金少于一个 lamport，则不对该账户作任何更改。

余额不足以支付租金的账户仅仅会无法加载。

一定比例的租金被销毁。 其余部分（即交易费）按质押权重，在每个 slot 结束的时候分配给验证节点账号。

最后，根据协议级别的帐户更新进行租金收取（就像向验证节点分发租金），这意味着没有相应的租金扣减交易。 因此，租金收取的过程其实非常隐秘，只能通过最近的交易或其账户地址前缀预先确定的时间来观察到。

## 设计考虑因素

### 当前设计依据

根据前面的设计，不可能有帐户处于遗漏、不会交互或者不支付租金的状态。 Accounts always pay rent exactly once for each epoch, except rent-exempt, sysvar and executable accounts.

This is an intended design choice. Otherwise, it would be possible to trigger unauthorized rent collection with `Noop` instruction by anyone who may unfairly profit from the rent (a leader at the moment) or save the rent given anticipated fluctuating rent cost.

As another side-effect of this choice, also note that this periodic rent collection effectively forces validators not to store stale accounts into a cold storage optimistically and save the storage cost, which is unfavorable for account owners and may cause transactions on them to stall longer than others. On the flip side, this prevents malicious users from creating significant numbers of garbage accounts, burdening validators.

As the overall consequence of this design, all accounts are stored equally as a validator's working set with the same performance characteristics, reflecting the uniform rent pricing structure.

### 特别收藏

考虑按需要收取租金\(即当帐户被加载/访问的时候\)。 采取这种办法的问题是：

- 某笔交易加载为“信用额度”的帐户可能会很合理地指望存在一个租金期限，

  但是任何这类交易都无法写入

- "打败忙碌”的机制\(即寻找需要支付租金的帐户\) 是可取的,

  不经常加载的帐户可以获得一些免费的机会

### 收取租金的系统说明

通过系统指示收取租金时需要注意，它会自然把租金分配给活跃和质押权重的节点，并且可以逐步进行。 然而：

- 它会对网络流量产生不利影响
- 该过程需要在运行时间之前进行特殊的套件处理，因为非系统程序所有者的帐户可能会被此指示扣除。
- 必须有人发布一笔交易
