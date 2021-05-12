---
title: Transaction Fees
---

**规则有可能发生变化。**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. 交易费在 Solana 经济设计中提供了许多好处，例如：

- 为验证者网络提供处理状态交易所需的 CPU/GPU 资源的单位补偿。
- 通过引入真实的交易成本来减少网络垃圾信息。
- 并通过协议捕获的每笔交易的最低费用金额为网络提供潜在的长期经济稳定性，如下所述。

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

当前许多区块链经济体(如比特币、以太坊)，在短期内依靠基于协议的奖励来支持经济，并假设通过交易费产生的收入将在长期内支持经济，当协议衍生的奖励到期时。 In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. 一个预定的全球通货膨胀率为通过上述过程分配给验证客户端的奖励提供了来源。

交易费由网络集群根据最近的历史吞吐量来设置，参见[拥堵驱动费用](implemented-proposals/transaction-fees.md#congestion-driven-fees)。 This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. 通过这种方式，协议可以使用最低费用来锁定所需的硬件利用率。 By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. 这个调整过程可以被认为是类似于比特币协议中的难度调整算法，不过在这种情况下，它是在调整最低交易费用，以引导交易处理硬件使用量达到预期水平。

如前所述，每笔交易费中都有固定比例要被销毁。 The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

此外，费用销毁也可以作为分叉选择的一个考虑因素。 在 PoH 分叉有一个恶意的、审查的领导者的情况下，由于审查所损失的费用，我们希望被破坏的总费用比可比的诚实分叉要少。 如果审查领导者要补偿这些损失的协议费，他们就必须自己替换掉自己分叉的费用销毁，从而有可能降低首先进行审查的动机。
