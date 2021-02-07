---
title: Adjusted Staking Yield
---


### Token Dilution

Similarly we can look at the expected *Staked Dilution* (i.e. *Adjusted Staking Yield*) and *Un-staked Dilution* as previously defined. Again, *dilution* in this context is defined as the change in fractional representation (i.e. ownership) of a set of tokens within a larger set. In this sense, dilution can be a positive value: an increase in fractional ownership (staked dilution / *Adjusted Staking Yield*), or a negative value: a decrease in fractional ownership (un-staked dilution).

We are interested in the relative change in ownership of staked vs un-staked tokens as the overall token pool increases with inflation issuance. As discussed, this issuance is distributed only to staked token holders, increasing the staked token fractional representation of the *Total Current Supply*.

Continuing with the same *Inflation Schedule* parameters as above, we see the fraction of staked supply grow as shown below.

![](/img/p_ex_staked_supply_w_range_initial_stake.png)

Due to this relative change in representation, the proportion of stake of any token holder will also change as a function of the *Inflation Schedule* and the proportion of all tokens that are staked.

Of initial interest, however, is the *dilution of **un-staked** tokens*, or $D_{us}$. In the case of un-staked tokens, token dilution is only a function of the *Inflation Schedule* because the amount of un-staked tokens doesn't change over time.

This can be seen by explicitly calculating un-staked dilution as $D_{us}$. The un-staked proportion of the token pool at time $t$ is $P_{us}(t_{N})$ and $I_{t}$ is the incremental inflation rate applied between any two consecutive time points. $SAFE_{us}(t)$ and $SAFE_{total}(t)$ is the amount of un-staked and total SAFE on the network, respectively, at time $t$. Therefore $P_{us}(t) = SAFE_{us}(t)/SAFE_{total}(t)$.

$$
\begin{aligned}
	D_{us} &= \left( \frac{P_{us}(t_{1}) - P_{us}(t_{0})}{P_{us}(t_{0})} \right)\\
		&= \left( \frac{ \left( \frac{SAFE_{us}(t_{2})}{SAFE_{total}(t_{2})} \right) - \left( \frac{SAFE_{us}(t_{1})}{SAFE_{total}(t_{1})} \right)}{ \left( \frac{SAFE_{us}(t_{1})}{SAFE_{total}(t_{1})} \right) } \right)\\

\end{aligned}
$$

However, because inflation issuance only increases the total amount and the un-staked supply doesn't change:

$$
\begin{aligned}
	SAFE_{us}(t_2) &= SAFE_{us}(t_1)\\
	SAFE_{total}(t_2) &= SAFE_{total}(t_1)\times (1 + I_{t_1})\\
\end{aligned}

$$

So $D_{us}$ becomes:

$$
\begin{aligned}
	D_{us} &= \left( \frac{ \left( \frac{SAFE_{us}(t_{1})}{SAFE_{total}(t_{1})\times (1 + I_{1})} \right) - \left( \frac{SAFE_{us}(t_{1})}{SAFE_{total}(t_{1})} \right)}{ \left( \frac{SAFE_{us}(t_{1})}{SAFE_{total}(t_{1})} \right) } \right)\\
	D_{us} &= \frac{1}{(1 + I_{1})} - 1\\
\end{aligned}
$$

Or generally, dilution for un-staked tokens over any time frame undergoing inflation $I$:

$$
D_{us} = -\frac{I}{I + 1} \\
$$

So as guessed, this dilution is independent of the total proportion of staked tokens and only depends on inflation rate. This can be seen with our example *Inflation Schedule* here:


![p_ex_unstaked_dilution](/img/p_ex_unstaked_dilution.png)


### Estimated Adjusted Staked Yield

We can do a similar calculation to determine the *dilution* of staked token holders, or as we've defined here as the ***Adjusted Staked Yield***, keeping in mind that dilution in this context is an *increase* in proportional ownership over time.  We'll use the terminology *Adjusted Staked Yield* to avoid confusion going forward.

To see the functional form, we calculate, $Y_{adj}$, or the *Adjusted Staked Yield* (to be compared to *D_{us}* the dilution of un-staked tokens above), where $P_{s}(t)$ is the staked proportion of token pool at time $t$ and $I_{t}$ is the incremental inflation rate applied between any two consecutive time points. The definition of $Y_{adj}$ is therefore:

$$
	Y_{adj} = \frac{P_s(t_2) - P_s(t_1)}{P_s(t_1)}\\
$$

As seen in the plot above, the proportion of staked tokens increases with inflation issuance. Letting $SAFE_s(t)$ and $SAFE_{\text{total}}(t)$ represent the amount of staked and total SAFE at time $t$ respectively:

$$
	P_s(t_2) = \frac{SAFE_s(t_1) + SAFE_{\text{total}}(t_1)\times I(t_1)}{SAFE_{\text{total}}(t_1)\times (1 + I(t_1))}\\
$$

Where $SAFE_{\text{total}}(t_1)\times I(t_1)$ is the additional inflation issuance added to the staked token pool. Now we can write $Y_{adj}$ in common terms $t_1 = t$:

$$
\begin{aligned}
Y_{adj} &= \frac{\frac{SAFE_s(t) + SAFE_{\text{total}}(t)\times I(t)}{SAFE_{\text{total}}(t)\times (1 + I(t))} - \frac{SAFE_s(t)}{SAFE_{\text{total}}(t)} }{ \frac{SAFE_s(t)}{SAFE_{\text{total}}(t)} }  \\
	&= \frac{ SAFE_{\text{total}}(t)\times (SAFE_s(t) + SAFE_{\text{total}}(t)\times I(t)) }{ SAFE_s(t)\times SAFE_{\text{total}}\times (1 + I(t)) } -1 \\
\end{aligned}
$$

which simplifies to:

$$
Y_{adj} =  \frac{ 1 + I(t)/P_s(t) }{ 1 + I(t) } - 1\\
$$

So we see that the *Adjusted Staked Yield* a function of the inflation rate and the percent of staked tokens on the network. We can see this plotted for various staking fractions here:


![p_ex_adjusted_staked_yields](/img/p_ex_adjusted_staked_yields.png)


It is also clear that in all cases, dilution of un-staked tokens $>$ adjusted staked yield (i.e. dilution of staked tokens). Explicitly we can look at the *relative dilution of un-staked tokens to staked tokens:* $D_{us}/Y_{adj}$. Here the relationship to inflation drops out and the relative dilution, i.e. the impact of staking tokens vs not staking tokens, is purely a function of the % of the total token supply staked. From above

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

Again, this represents the change in fractional change in ownership of staked tokens and illustrates the built-in incentive for token holder to stake their tokens to earn *Staked Yield* and avoid *Un-staked Dilution*.
