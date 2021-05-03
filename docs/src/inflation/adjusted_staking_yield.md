---
title: Adjusted Staking Yield
---

### Token Dilution

Similarly we can look at the expected _Staked Dilution_ (i.e. _Adjusted Staking Yield_) and _Un-staked Dilution_ as previously defined. Again, _dilution_ in this context is defined as the change in fractional representation (i.e. ownership) of a set of tokens within a larger set. In this sense, dilution can be a positive value: an increase in fractional ownership (staked dilution / _Adjusted Staking Yield_), or a negative value: a decrease in fractional ownership (un-staked dilution).

We are interested in the relative change in ownership of staked vs un-staked tokens as the overall token pool increases with inflation issuance. As discussed, this issuance is distributed only to staked token holders, increasing the staked token fractional representation of the _Total Current Supply_.

Continuing with the same _Inflation Schedule_ parameters as above, we see the fraction of staked supply grow as shown below.

![](/img/p_ex_staked_supply_w_range_initial_stake.png)

Due to this relative change in representation, the proportion of stake of any token holder will also change as a function of the _Inflation Schedule_ and the proportion of all tokens that are staked.

Of initial interest, however, is the _dilution of **un-staked** tokens_, or $D_{us}$. In the case of un-staked tokens, token dilution is only a function of the _Inflation Schedule_ because the amount of un-staked tokens doesn't change over time.

This can be seen by explicitly calculating un-staked dilution as $D_{us}$. The un-staked proportion of the token pool at time $t$ is $P_{us}(t_{N})$ and $I_{t}$ is the incremental inflation rate applied between any two consecutive time points. $SOL_{us}(t)$ and $SOL_{total}(t)$ is the amount of un-staked and total SOL on the network, respectively, at time $t$. Therefore $P_{us}(t) = SOL_{us}(t)/SOL_{total}(t)$.

$$
\begin{aligned}
	D_{us} &= \left( \frac{P_{us}(t_{1}) - P_{us}(t_{0})}{P_{us}(t_{0})} \right)\\
		&= \left( \frac{ \left( \frac{SOL_{us}(t_{2})}{SOL_{total}(t_{2})} \right) - \left( \frac{SOL_{us}(t_{1})}{SOL_{total}(t_{1})} \right)}{ \left( \frac{SOL_{us}(t_{1})}{SOL_{total}(t_{1})} \right) } \right)\\

\end{aligned}
$$

However, because inflation issuance only increases the total amount and the un-staked supply doesn't change:

$$
\begin{aligned}
	SOL_{us}(t_2) &= SOL_{us}(t_1)\\
	SOL_{total}(t_2) &= SOL_{total}(t_1)\times (1 + I_{t_1})\\
\end{aligned}
$$

So $D_{us}$ becomes:

$$
\begin{aligned}
	D_{us} &= \left( \frac{ \left( \frac{SOL_{us}(t_{1})}{SOL_{total}(t_{1})\times (1 + I_{1})} \right) - \left( \frac{SOL_{us}(t_{1})}{SOL_{total}(t_{1})} \right)}{ \left( \frac{SOL_{us}(t_{1})}{SOL_{total}(t_{1})} \right) } \right)\\
	D_{us} &= \frac{1}{(1 + I_{1})} - 1\\
\end{aligned}
$$

Or generally, dilution for un-staked tokens over any time frame undergoing inflation $I$:

$$
D_{us} = -\frac{I}{I + 1} \\
$$

So as guessed, this dilution is independent of the total proportion of staked tokens and only depends on inflation rate. This can be seen with our example _Inflation Schedule_ here:

![p_ex_unstaked_dilution](/img/p_ex_unstaked_dilution.png)

### Estimated Adjusted Staked Yield

We can do a similar calculation to determine the _dilution_ of staked token holders, or as we've defined here as the **_Adjusted Staked Yield_**, keeping in mind that dilution in this context is an _increase_ in proportional ownership over time. We'll use the terminology _Adjusted Staked Yield_ to avoid confusion going forward.

To see the functional form, we calculate, $Y_{adj}$, or the _Adjusted Staked Yield_ (to be compared to _D\_{us}_ the dilution of un-staked tokens above), where $P_{s}(t)$ is the staked proportion of token pool at time $t$ and $I_{t}$ is the incremental inflation rate applied between any two consecutive time points. The definition of $Y_{adj}$ is therefore:

$$
	Y_{adj} = \frac{P_s(t_2) - P_s(t_1)}{P_s(t_1)}\\
$$

As seen in the plot above, the proportion of staked tokens increases with inflation issuance. Letting $SOL_s(t)$ and $SOL_{\text{total}}(t)$ represent the amount of staked and total SOL at time $t$ respectively:

$$
	P_s(t_2) = \frac{SOL_s(t_1) + SOL_{\text{total}}(t_1)\times I(t_1)}{SOL_{\text{total}}(t_1)\times (1 + I(t_1))}\\
$$

Where $SOL_{\text{total}}(t_1)\times I(t_1)$ is the additional inflation issuance added to the staked token pool. Now we can write $Y_{adj}$ in common terms $t_1 = t$:

$$
\begin{aligned}
Y_{adj} &= \frac{\frac{SOL_s(t) + SOL_{\text{total}}(t)\times I(t)}{SOL_{\text{total}}(t)\times (1 + I(t))} - \frac{SOL_s(t)}{SOL_{\text{total}}(t)} }{ \frac{SOL_s(t)}{SOL_{\text{total}}(t)} }  \\
	&= \frac{ SOL_{\text{total}}(t)\times (SOL_s(t) + SOL_{\text{total}}(t)\times I(t)) }{ SOL_s(t)\times SOL_{\text{total}}\times (1 + I(t)) } -1 \\
\end{aligned}
$$

which simplifies to:

$$
Y_{adj} =  \frac{ 1 + I(t)/P_s(t) }{ 1 + I(t) } - 1\\
$$

So we see that the _Adjusted Staked Yield_ a function of the inflation rate and the percent of staked tokens on the network. We can see this plotted for various staking fractions here:

![p_ex_adjusted_staked_yields](/img/p_ex_adjusted_staked_yields.png)

It is also clear that in all cases, dilution of un-staked tokens $>$ adjusted staked yield (i.e. dilution of staked tokens). Explicitly we can look at the _relative dilution of un-staked tokens to staked tokens:_ $D_{us}/Y_{adj}$. Here the relationship to inflation drops out and the relative dilution, i.e. the impact of staking tokens vs not staking tokens, is purely a function of the % of the total token supply staked. From above

$$
\begin{aligned}
Y_{adj} &=  \frac{ 1 + I/P_s }{ 1 + I } - 1,~\text{and}\\
D_{us} &= -\frac{I}{I + 1},~\text{so} \\
\frac{D_{us}}{Y_{adj}} &= \frac{ \frac{I}{I + 1} }{ \frac{ 1 + I/P_s }{ 1 + I } - 1 } \\
\end{aligned}
$$

which simplifies as,

$$
	\begin{aligned}
	\frac{D_{us}}{Y_{adj}} &= \frac{ I }{ 1 + \frac{I}{P_s} - (1 + I)}\\
	&= \frac{ I }{ \frac{I}{P_s} - I}\\
	\frac{D_{us}}{Y_{adj}}&= \frac{ P_s }{ 1 - P_s}\\
	\end{aligned}
$$

Where we can see a primary dependence of the relative dilution of un-staked tokens to staked tokens is on the function of the proportion of total tokens staked. As shown above, the proportion of total tokens staked changes over time (i.e. $P_s = P_s(t)$ due to the re-staking of inflation issuance thus we see relative dilution grow over time as:

![p_ex_relative_dilution](/img/p_ex_relative_dilution.png)

As might be intuitive, as the total fraction of staked tokens increases the relative dilution of un-staked tokens grows dramatically. E.g. with $80\%$ of the network tokens staked, an un-staked token holder will experience ~$400\%$ more dilution than a staked holder.

Again, this represents the change in fractional change in ownership of staked tokens and illustrates the built-in incentive for token holder to stake their tokens to earn _Staked Yield_ and avoid _Un-staked Dilution_.
