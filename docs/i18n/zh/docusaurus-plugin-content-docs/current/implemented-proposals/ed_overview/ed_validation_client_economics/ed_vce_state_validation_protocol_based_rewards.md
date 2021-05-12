---
title: 通货膨胀规划
---

**规则有可能发生变化。 请在 Solana 论坛上关注最近的经济讨论：https://forums.Solana.com。**

验证节点客户端在 Solana 网络中具有两种功能作用。

- 验证节点对他们观察到的 PoH 的当前全球状态进行\(投票\)。
- 验证节点在利益加权的循环计划中被选为 "领导者"，在此期间，他们负责收集未完成的交易，并将其纳入其观察到的 PoH 中，从而更新网络的全球状态，并提供区块链的连续性。

验证节点客户端对这些服务的奖励将在每个 Solana 纪元结束时分配。 如前所述，验证节点-客户的报酬是通过基于协议的年度通货膨胀率收取的佣金来提供的，该佣金按照每个验证节点节点的质押权重比例分配(见下文)，同时还包括每次领导者轮换期间可用的领导者主张的交易费用。 举例说明 即在给定的验证节点-客户端被选为领导者期间，它有机会保留每笔交易费的一部分，减去协议规定的被销毁的金额 (见[验证节点-客户端状态交易费](ed_vce_state_validation_transaction_fees.md))。

验证节点客户端收到的协议级别有效质押收益率/(%/)，每一个纪元将是以下因素的函数：

- 验证节点当前的全局通货膨胀率，由预先确定的去通货膨胀发行计划表推导出来的(见[验证客户端经济学](ed_vce_overview.md))。
- 验证节点当前总循环供应量中，质押的 SOL 占比。
- 验证节点服务收取的佣金。
- 验证节点在上一个纪元中，给定验证节点的在线/参与\[的投票 %\]。

第一个因素仅是协议参数的函数\(即独立于验证节点在给定纪元中的行为\)，其结果是设计了一个膨胀时间表，以激励早期参与，提供明确的货币稳定性，并在网络中提供最佳的安全性。

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

具体而言：

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

使用这些范围来模拟一些可能的通货膨胀表，我们可以探索一段时间内的通货膨胀：

![](/img/p_inflation_schedule_ranges_w_comments.png)

在上图中，确定了范围的平均值，以说明每个参数的贡献。 From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}} CONTEXT

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. 在下面的例子，我们选择了上面探讨的参数范围的中间值：

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### 调整后的质押收益

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. 即 通货膨胀的正向稀释效应。

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. 我们可以在这里看到各种质押占比的情况。

![](/img/p_ex_staked_dilution.png)
