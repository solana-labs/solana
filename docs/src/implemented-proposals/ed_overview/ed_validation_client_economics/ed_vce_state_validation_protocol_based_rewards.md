---
title: Inflation Schedule
---

**Subject to change.**

Validator-clients have two functional roles in the Solana network:

- Validate \(vote\) the current global state of their observed PoH.
- Be elected as ‘leader’ on a stake-weighted round-robin schedule during which time they are responsible for collecting outstanding transactions and incorporating them into their observed PoH, thus updating the global state of the network and providing chain continuity.

Validator-client rewards for these services are to be distributed at the end of each Solana epoch. As previously discussed, compensation for validator-clients is provided via a commission charged on the protocol-based annual inflation rate dispersed in proportion to the stake-weight of each validator-node \(see below\) along with leader-claimed transaction fees available during each leader rotation. I.e. during the time a given validator-client is elected as leader, it has the opportunity to keep a portion of each transaction fee, less a protocol-specified amount that is destroyed \(see [Validation-client State Transaction Fees](ed_vce_state_validation_transaction_fees.md)\).

The effective protocol-based annual staking yield \(%\) per epoch received by validation-clients is to be a function of:

- the current global inflation rate, derived from the pre-determined disinflationary issuance schedule \(see [Validation-client Economics](ed_vce_overview.md)\)
- the fraction of staked SOLs out of the current total circulating supply,
- the commission charged by the validation service,
- the up-time/participation \[% of available slots that validator had opportunity to vote on\] of a given validator over the previous epoch.

The first factor is a function of protocol parameters only \(i.e. independent of validator behavior in a given epoch\) and results in an inflation schedule designed to incentivize early participation, provide clear monetary stability and provide optimal security in the network.

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

Specifically:

- _Initial Inflation Rate_: 7-9%
- _Disinflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

Using these ranges to simulate a number of possible Inflation Schedules, we can explore inflation over time:

![](/img/p_inflation_schedule_ranges_w_comments.png)

In the above graph, the average values of the range are identified to illustrate the contribution of each parameter.
From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

$$
\%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}
$$

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. For the below example, we’ve chosen the middle of the parameter ranges explored above:

- _Initial Inflation Rate_: 8%
- _Disinflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### Adjusted Staking Yield

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. I.e. the positive dilutive effects of inflation.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. We can see this plotted for various staking fractions here:

![](/img/p_ex_staked_dilution.png)
