### 2.2 State-validation Transaction Fees

Each message sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, must contain a transaction fee. Transaction fees offer many benefits in the Solana economic design, for example they:

* provide unit compensation to the validator network for the CPU/GPU resources necessary to process the state transaction,

* reduce network spam by introducing real cost to transactions,

* open avenues for a transaction market to incentivize validation-client to collect and process submitted transactions in their function as leader,

* and provide potential long-term economic stability of the network through a protocol-captured minimum fee amount per transaction, as described below.

Many current blockchain economies (e.g. Bitcoin, Ethereum), rely on protocol-based rewards to support the economy in the short term, with the assumption that the revenue generated through transaction fees will support the economy in the long term, when the protocol derived rewards expire. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion of each transaction fee is sent to the mining pool, with the resulting fee going to the current leader processing the transaction. These pooled fees, then re-enter the system through rewards distributed to validation-clients, through the process described above, and replication-clients, as discussed below.

The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing a redistribution avenue that protects against "tax evasion" attacks (i.e. side-channel fee payments)1. Constraints on the fixed portion of transaction fees going to the mining pool, to establish long-term economic sustainability, are established and discussed in detail in [Section 4](ed_economic_sustainability.md).
