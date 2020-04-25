# Cluster Economics

**Subject to change.**

Solana’s crypto-economic system is designed to promote a healthy, long term self-sustaining economy with participant incentives aligned to the security and decentralization of the network. The main participants in this economy are validation-clients. Their contributions to the network, state validation, and their requisite incentive mechanisms are discussed below.

The main channels of participant remittances are referred to as protocol-based rewards and transaction fees. Protocol-based rewards are issuances from a global, protocol-defined, inflation rate. These rewards will constitute the total reward delivered to validation clients, the remaining sourced from transaction fees. In the early days of the network, it is likely that protocol-based rewards, deployed based on predefined issuance schedule, will drive the majority of participant incentives to participate in the network.

These protocol-based rewards, to be distributed to participating validation clients, are to be a result of a global supply inflation rate, calculated per Solana epoch and distributed amongst the active validator set. As discussed further below, the per annum inflation rate is based on a pre-determined disinflationary schedule. This provides the network with monetary supply predictability which supports long term economic stability and security.

Transaction fees are market-based participant-to-participant transfers, attached to network interactions as a necessary motivation and compensation for the inclusion and execution of a proposed transaction. A mechanism for long-term economic stability and forking protection through partial burning of each transaction fee is also discussed below.

A high-level schematic of Solana’s crypto-economic design is shown below in **Figure 1**. The specifics of validation-client economics are described in sections: [Validation-client Economics](ed_validation_client_economics/README.md), [State-validation Protocol-based Rewards](ed_validation_client_economics/ed_vce_state_validation_protocol_based_rewards.md), [State-validation Transaction Fees](ed_validation_client_economics/ed_vce_state_validation_transaction_fees.md). Also, the section titled [Validation Stake Delegation](ed_validation_client_economics/ed_vce_validation_stake_delegation.md) closes with a discussion of validator delegation opportunities and marketplace. Additionally, in [Storage Rent Economics](ed_storage_rent_economics.md), we describe an implementation of storage rent to account for the externality costs of maintaining the active state of the ledger. An outline of features for an MVP economic design is discussed in the [Economic Design MVP](ed_mvp.md) section.

![](../../.gitbook/assets/economic_design_infl_230719.png)

**Figure 1**: Schematic overview of Solana economic incentive design.
