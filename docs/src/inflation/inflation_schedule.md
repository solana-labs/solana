---
title: Solana's Proposed Inflation Schedule
---

As mentioned above, the network's *Inflation Schedule* is uniquely described by three parameters: *Initial Inflation Rate*, *Dis-inflation Rate* and *Long-term Inflation Rate*. When considering these numbers, there are many factors to take into account:

- A large portion of the SOL issued via inflation will be distributed to stake-holders in proportion to the SOL they have staked. We want to ensure that the *Inflation Schedule* design results in reasonable *Staking Yields* for token holders who delegate SOL and for validation service providers (via commissions taken from *Staking Yields*).
- The primary driver of *Staked Yield* is the amount of SOL staked divided by the total amount of SOL (% of total SOL staked). Therefore the distribution and delegation of tokens across validators are important factors to understand when determining initial inflation parameters.
- [Yield throttling](https://forums.solana.com/t/validator-yield-throttling-proposal-discussion/855/5) is a current area or research that would impact *staking-yields*. This is not taken into consideration in the discussion here or the modeling below.
- Overall token issuance - i.e. what do we expect the Current Total Supply to be in 10 years, or 20 years?
- Long-term, steady-state inflation is an important consideration not only for sustainable support for the validator ecosystem and the Solana Foundation grant programs, but also should be tuned in consideration with expected token losses and burning over time.
- The rate at which we expect network usage to grow, as a consideration to the dis-inflationary rate. Over time, we plan for inflation to drop and expect that usage will grow.

Based on these considerations and the community discussions following the initial [design](https://forums.solana.com/t/solana-inflation-design-overview/920), the Solana Foundation proposes the following Inflation Schedule parameters:

- Initial Inflation Rate: $8\%$
- Dis-inflation Rate: $-15\%$
- Long-term Inflation Rate: $1.5\%$

These parameters define the proposed *Inflation Schedule*. Below we show implications of these parameters. These plots only show the impact of inflation issuances given the Inflation Schedule as parameterized above. They *do not account* for other factors that may impact the Total Supply such as fee/rent burning, slashing or other unforeseen future token destruction events. Therefore, what is presented here is an **upper limit** on the amount of SOL issued via inflation.

![](/img/p_inflation_schedule.png)

In the above graph we see the annual inflation rate [$\%$] over time, given the inflation parameters proposed above.

![](/img/p_total_supply.png)

Similarly, here we see the *Total Current Supply* of SOL [MM] over time, assuming an initial *Total Current Supply* of `488,587,349 SOL` (i.e. for this example, taking the *Total Current Supply* as of `2020-01-25` and simulating inflation starting from that day).

Setting aside validator uptime and commissions, the expected Staking Yield and Adjusted Staking Yield metrics are then primarily a function of the % of total SOL staked on the network. Therefore we can we can model *Staking Yield*, if we introduce an additional parameter *% of Staked SOL*:

$$
\%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}
$$

This parameter must be estimated because it is a dynamic property of the token holders and staking incentives. The values of *% of Staked SOL* presented here range from $60\% - 90\%$, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example *Staked Yield* that a staker might expect over time on the Solana network with the *Inflation Schedule* as specified. This is an idealized *Staked Yield* as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling  and potential slashing incidents. It additionally ignores that *% of Staked SOL* is dynamic by design - the economic incentives set up by this *Inflation Schedule* are more clearly seen when *Token Dilution* is taken into account (see the **Adjusted Staking Yield** section below).
