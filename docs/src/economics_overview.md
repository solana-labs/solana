---
title: Solana Economics Overview
---

**Subject to change.**

Solana’s crypto-economic system is designed to promote a healthy, long term self-sustaining economy with participant incentives aligned to the security and decentralization of the network. The main participants in this economy are validation-clients. Their contributions to the network, state validation, and their requisite incentive mechanisms are discussed below.

The main channels of participant remittances are referred to as
protocol-based rewards and transaction fees. Protocol-based rewards
are generated from inflationary issuances from a protocol-defined inflation schedule. These rewards will constitute the total protocol-based reward delivered to validation clients, the remaining sourced from transaction fees. In the early days of the network, it is likely that protocol-based rewards, deployed based on predefined issuance schedule, will drive the majority of participant incentives to participate in the network.

These protocol-based rewards are calculated per epoch and distributed across the active
delegated stake and validator set (per validator commission). As discussed further below, the per annum inflation rate is based on a pre-determined disinflationary schedule. This provides the network with supply predictability which supports long term economic stability and security.

Transaction fees are participant-to-participant transfers, attached to network interactions as a motivation and compensation for the inclusion and execution of a proposed transaction. A mechanism for long-term economic stability and forking protection through partial burning of each transaction fee is also discussed below.

First, an overview of the inflation design is presented. This section starts with defining and clarifying [Terminology](inflation/terminology.md) commonly used subsequently in the discussion of inflation and the related components. Following that, we outline Solana's proposed [Inflation Schedule](inflation/inflation_schedule.md), i.e. the specific parameters that uniquely parameterize the protocol-driven inflationary issuance over time. Next is a brief section on [Adjusted Staking Yield](inflation/adjusted_staking_yield.md), and how token dilution might influence staking behavior.

An overview of [Transaction Fees](transaction_fees.md) on Solana is followed by a discussion of [Storage Rent Economics](storage_rent_economics.md) in which we describe an implementation of storage rent to account for the externality costs of maintaining the active state of the ledger.
