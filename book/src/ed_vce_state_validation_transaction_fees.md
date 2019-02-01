### State-validation Transaction Fees

Each message sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, must contain a transaction fee. Transaction fees offer many benefits in the Solana economic design, for example they:

* provide unit compensation to the validator network for the CPU/GPU resources necessary to process the state transaction,

* reduce network spam by introducing real cost to transactions,

* open avenues for a transaction market to incentivize validation-client to collect and process submitted transactions in their function as leader,

* and provide potential long-term economic stability of the network through a protocol-captured minimum fee amount per transaction, as described below.

Many current blockchain economies (e.g. Bitcoin, Ethereum), rely on protocol-based rewards to support the economy in the short term, with the assumption that the revenue generated through transaction fees will support the economy in the long term, when the protocol derived rewards expire. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion of each transaction fee is sent to the mining pool, with the resulting fee going to the current leader processing the transaction. These pooled fees, then re-enter the system primarily as rewards for validated proofs-of-replication submitted by replication-clients. This mechanism provides a path to a long-term self-sustainable economy where replication-clients are incentivized through this 'captured' minimum fee and validation clients are incentivized through the transaction fees remaining. 

The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing a redistribution avenue that protects against "tax evasion" attacks (i.e. side-channel fee payments)<sup>[1](ed_referenced.md)</sup>. Constraints on the fixed portion of transaction fees going to the mining pool, to establish long-term economic sustainability, are established and discussed in detail in the [Economic Sustainability](ed_economic_sustainability.md) section.

This minimum, protocol-earmarked, portion of each transaction fee can be dynamically adjusted depending on historical gas usage. In this way, the protocol can use the minimum fee to target a desired hardware utilisation. By monitoring a protocol specified gas usage with respect to a desired, target usage amount (e.g. 50% of a block's capacity), the minimum fee can be raised/lowered which should, in turn, lower/raise the actual gas usage per block until it reaches the target amount. This adjustment process can be thought of as similar to the difficulty adjustment algorithm in the Bitcoin protocol, however in this case it is adjusting the minimum transaction fee to guide the transaction processing hardware usage to a desired level.

Additionally, the minimum protocol captured fee can be a consideration in fork selection. In the case of a PoH fork with a  malicious, censoring leader, we would expect the total procotol captured fee to be less than a comparable honest fork, due to the fees lost from censoring. If the censoring leader is to compensate for these lost protocol fees, they would have to replace the fees on their fork themselves, thus potentially reducing the incentive to censor in the first place. 


