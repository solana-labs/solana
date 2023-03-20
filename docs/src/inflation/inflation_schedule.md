---
title: Solana's Proposed Inflation Schedule
---

As mentioned above, the network's _Inflation Schedule_ is uniquely described by three parameters: _Initial Inflation Rate_, _Disinflation Rate_ and _Long-term Inflation Rate_. When considering these numbers, there are many factors to take into account:

- A large portion of the SOL issued via inflation will be distributed to stake-holders in proportion to the SOL they have staked. We want to ensure that the _Inflation Schedule_ design results in reasonable _Staking Yields_ for token holders who delegate SOL and for validation service providers (via commissions taken from _Staking Yields_).
- The primary driver of _Staked Yield_ is the amount of SOL staked divided by the total amount of SOL (% of total SOL staked). Therefore the distribution and delegation of tokens across validators are important factors to understand when determining initial inflation parameters.
- Yield throttling is a current area of research that would impact _staking-yields_. This is not taken into consideration in the discussion here or the modeling below.
- Overall token issuance - i.e. what do we expect the Current Total Supply to be in 10 years, or 20 years?
- Long-term, steady-state inflation is an important consideration not only for sustainable support for the validator ecosystem and the Solana Foundation grant programs, but also should be tuned in consideration with expected token losses and burning over time.
- The rate at which we expect network usage to grow, as a consideration to the disinflationary rate. Over time, we plan for inflation to drop and expect that usage will grow.

Based on these considerations and the community discussions following the initial design, the Solana Foundation proposes the following Inflation Schedule parameters:

- Initial Inflation Rate: $8\%$
- Disinflation Rate: $-15\%$
- Long-term Inflation Rate: $1.5\%$

These parameters define the proposed _Inflation Schedule_. Below we show implications of these parameters. These plots only show the impact of inflation issuances given the Inflation Schedule as parameterized above. They _do not account_ for other factors that may impact the Total Supply such as fee/rent burning, slashing or other unforeseen future token destruction events. Therefore, what is presented here is an **upper limit** on the amount of SOL issued via inflation.

![](/img/p_inflation_schedule.png)

In the above graph we see the annual inflation rate [$\%$] over time, given the inflation parameters proposed above.

![](/img/p_total_supply.png)

Similarly, here we see the _Total Current Supply_ of SOL [MM] over time, assuming an initial _Total Current Supply_ of `488,587,349 SOL` (i.e. for this example, taking the _Total Current Supply_ as of `2020-01-25` and simulating inflation starting from that day).

Setting aside validator uptime and commissions, the expected Staking Yield and Adjusted Staking Yield metrics are then primarily a function of the % of total SOL staked on the network. Therefore we can we can model _Staking Yield_, if we introduce an additional parameter _% of Staked SOL_:

$$
\%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}
$$

This parameter must be estimated because it is a dynamic property of the token holders and staking incentives. The values of _% of Staked SOL_ presented here range from $60\% - 90\%$, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_ are more clearly seen when _Token Dilution_ is taken into account (see the **Adjusted Staking Yield** section below).
