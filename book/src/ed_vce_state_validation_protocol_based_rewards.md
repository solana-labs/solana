### 2.1 State-validation protocol-based rewards

Validator-clients have two functional roles in the Solana network

* Validate (vote) the current global state of that PoH along with any Proofs-of-Replication (see [Section 3](ed_replication_client_economics.md)) that they are eligible to validate

* Be elected as ‘leader’ on a stake-weighted round-robin schedule during which time they are responsible for collecting outstanding transactions and Proofs-of-Replication and incorporating them into the PoH, thus updating the global state of the network and providing chain continuity.

Validator-client rewards for these services are to be distributed at the end of each Solana epoch. Compensation for validator-clients is provided via a protocol-based annual interest rate dispersed in proportion to the stake-weight of each validator (see below) along with leader-claimed transaction fees available during each leader rotation. I.e. during the time a given validator-client is elected as leader, it has the opportunity to keep a portion of each non-PoRep transaction fee, less a protocol-specified amount that is returned to the mining pool (see [Section 2.2](ed_vce_state_validation_transaction_fees.md)). PoRep transaction fees are not collected directly by the leader client but pooled and returned to the validator set in proportion to the number of successfully validated PoReps. (see [Section 2.3](ed_vce_replication_validation_transaction_fees.md))

The protocol-based annual interest-rate (%) per epoch to be distributed to validation-clients is to be a function of:

* the current fraction of staked SOLs out of the current total circulating supply,

* the global time since the genesis block instantiation

* the up-time/participation [% of available slots/blocks that validator had opportunity to vote on?] of a given validator over the previous epoch.

The first two factors are protocol parameters only (i.e. independent of validator behavior in a given epoch) and describe a global validation reward schedule designed to both incentivize early participation and optimal security in the network. This schedule sets a maximum annual validator-client interest rate per epoch.

At any given point in time, this interest rate is pegged to a defined value given a specific % staked SOL out of the circulating supply (e.g. 10% interest rate when 66% of circulating SOL is staked). The interest rate adjusts as the square-root [TBD] of the % staked, leading to higher validation-client interest rates as the % staked drops below the targeted goal, thus incentivizing more participation leading to more security in the network. An example of such a schedule, for a specified point in time (e.g. network launch) is shown in **Table 1**.

| Percentage circulating supply staked [%] | Annual validator-client interest rate [%] |
| ---:    | ---:      |
| 5       | 13.87     |
| 15      | 13.31     |
| 25      | 12.73     |
| 35      | 12.12     |
| 45      | 11.48     |
| 55      | 10.80     |
| **66**  | **10.00** |
| 75      | 9.29      |
| 85      | 8.44      |    

**Table 1:** Example interest rate schedule based on % SOL staked out of circulating supply. In this case, interest rates are fixed at 10% for 66% of staked circulating supply

Over time, the interest rate, at any network staked percentage, will drop as described by an algorithmic schedule. Validation-client interest rates are designed to be higher in the early days of the network to incentivize participation and jumpstart the network economy. This mining-pool provided interest rate will reduce over time until a network-chosen baseline value is reached. This is a fixed, long-term, interest rate to be provided to validator-clients. This value does not represent the total interest available to validator-clients as transaction fees for both state-validation and ledger storage replication (PoReps) are not accounted for here. A validation-client interest rate schedule as a function of % network staked and time is shown in** Figure 2**.

![image alt text](img/image_1.png)

**Figure 2:** In this example schedule, the annual interest rate [%] reduces at around 16.7% per year, until it reaches the long-term, fixed, 4% rate.

This epoch-specific protocol-defined interest rate sets an upper limit of *protocol-generated* annual interest rate (not absolute total interest rate) possible to be delivered to any validator-client per epoch. The distributed interest rate per epoch is then discounted from this value based on the participation of the validator-client during the previous epoch. Each epoch is comprised of XXX slots. The protocol-defined interest rate is then discounted by the log [TBD] of the % of slots a given validator submitted a vote on a PoH branch during that epoch, see **Figure XX**
