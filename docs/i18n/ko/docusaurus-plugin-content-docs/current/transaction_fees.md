---
title: Transaction Fees
---

**Subject to change.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. Transaction fees offer many benefits in the Solana economic design, for example they:

- provide unit compensation to the validator network for the CPU/GPU resources necessary to process the state transaction,
- reduce network spam by introducing real cost to transactions,
- and provide potential long-term economic stability of the network through a protocol-captured minimum fee amount per transaction, as described below.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

Many current blockchain economies \(e.g. Bitcoin, Ethereum\), rely on protocol-based rewards to support the economy in the short term, with the assumption that the revenue generated through transaction fees will support the economy in the long term, when the protocol derived rewards expire. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. A scheduled global inflation rate provides a source for rewards distributed to validation-clients, through the process described above.

Transaction fees are set by the network cluster based on recent historical throughput, see [Congestion Driven Fees](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. In this way, the protocol can use the minimum fee to target a desired hardware utilization. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. This adjustment process can be thought of as similar to the difficulty adjustment algorithm in the Bitcoin protocol, however in this case it is adjusting the minimum transaction fee to guide the transaction processing hardware usage to a desired level.

As mentioned, a fixed-proportion of each transaction fee is to be destroyed. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

Additionally, the burnt fees can be a consideration in fork selection. In the case of a PoH fork with a malicious, censoring leader, we would expect the total fees destroyed to be less than a comparable honest fork, due to the fees lost from censoring. If the censoring leader is to compensate for these lost protocol fees, they would have to replace the burnt fees on their fork themselves, thus potentially reducing the incentive to censor in the first place.
