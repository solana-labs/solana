---
title: Validation-client Economics
---

**Subject to change.**

Validator-clients are eligible to receive protocol-based \(i.e. inflation-based\) rewards issued via stake-based annual interest rates \(calculated per epoch\) by providing compute \(CPU+GPU\) resources to validate and vote on a given PoH state. As discussed in the [Inflation Design](inflation/inflation_schedule.md) section, , these protocol-based rewards are determined through an algorithmic disinflationary schedule as a function of total amount of circulating tokens. The network is expected to launch with an annual inflation rate of 8%%, set to decrease by 15% per year until a long-term stable rate of 1.5% is reached. These issuances are to be distributed across the delegated stake accounts on the network, while validators have the opportunity to set a commission rate on their delegations. Because the network will be distributing a fixed amount of inflation rewards across the stake-weighted stake account set, any individual validator's revenue will be a function of the amount of staked SOL in relation to the circulating SOL and it's chosen commission rate.

Additionally, validator clients may earn revenue through fees via state-validation transactions. For clarity, we separately describe the design and motivation of these revenue distributions for validation-clients below: state-validation protocol-based rewards and state-validation transaction fees and rent.
