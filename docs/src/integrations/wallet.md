---
title: Wallet Integration Guide
---

This guide describes how to add support for Solana to your wallet.

_For help on integrating Solana wallets with your web application, check out the
[Solana wallet adapter](https://github.com/solana-labs/wallet-adapter)._

## Transaction Signing

In the most minimal sense, a wallet is an application allows signing messages.
On Solana, a transaction consists of two parts: a list of signatures and a
message containing instructions to be processed by on-chain programs.  Visit the
Transactions page to dive deeper into how transactions and transaction messages
are serialized.

Despite the fact that Solana transactions can contain multiple signatures,
wallets are only responsible for setting the signature for the keypair that they
manage. Other transaction signatures can be created and added independently and
typically are used to create ephemeral accounts or to allow another party to
cover transaction fees.

<!-- TODO: web3.js example -->

## Transaction Details

When wallets prompt users to sign a message, they are responsible for giving as
much context as possible to help the user decide on whether it's in their best
interest to sign or not.

### Transaction Fees

To display the transaction fee to the user, your wallet should send the
serialized transaction message to the RPC API `getFeeForMessage`. Solana
transaction fees are deterministic for a signed transaction message but that
determinism is tied to the message's embedded blockhash. The RPC API will lookup
the fee parameters for the message's blockhash and use those fee parameters to
calculate the message's total fees. This also helps ensure forward compatibility
is maintained as new types of fees are introduced. There are currently two
types of fees used on Solana: the base congestion fee and the compute unit
prioritization fee and each should be shown to the user separately.

<!-- TODO: web3.js example -->

#### Base Congestion Fee

Starting from the genesis of the Solana blockchain, transactions have been
charged a 5000 lamport fee per signature. The original plan for this fee was to
have it fluctuate as demand for blockspace increased or decreased over time to
combat congestion. Currently the dynamic congestion fee pricing is not enabled.

#### Compute Unit Prioritization Fee

With the introduction of the "Compute Budget" native program, transactions also
have the ability to set a prioritization fee which is tied to the compute unit
limit of a transaction. Transactions may include "Compute Budget" program
instructions to set a compute unit limit and compute unit price. If transactions
set competitive values in these instructions, they will economically incentivize
block producers to process their transaction sooner, assuming that block
producers include transactions in order of highest fee per compute unit. The
prioritization fee is the product of the compute unit limit and the compute unit
price.

### Transaction Expiration

Solana transactions expire by default because their embedded blockhash is only
valid for a limited amount of time. Blockhashes currently expire after 151 slots
have elapsed since the slot which corresponds to the transaction's blockhash.
However, in the near future, blockhashes will expire after reaching a block
height of 300. It's recommended that your wallet uses block height to display
transaction expiration to avoid incorrectly informing a user that their
transaction has expired when it is still live.

It's not currently possible to fetch the block height of a transaction's
blockhash. But wallets may choose to fetch a blockhash from the
`getLatestBlockhash` RPC API which returns the last valid block height of the
returned blockhash. Then if your wallet replaces the transaction's blockhash, it
will know the block height when the transaction expires and can poll the
`getBlockHeight` RPC API to show a countdown of how many blocks remain until the
transaction expires.

<!-- TODO: web3.js example -->

### Transaction Instructions

Each transaction instruction consists of 3 parts: a program id, a list of
account inputs, and the instruction data. Most Solana programs are built with
the Anchor framework which emits an IDL that is stored on-chain and can be used
to get human readable descriptions for the program, account inputs, and
instruction details. Use the Anchor documentation to learn more.

## Transaction Modifications

Wallets can take a more active role in transaction construction to improve
transaction processing.

### Improve Prioritization with Compute Budget Instructions

Solana validators are incentivized to prioritize transactions which pay relatively
higher fees for the amount of resources consumed during processing. In order to
help users get their transactions processed in a block in a timely matter, wallets
should add compute budget instructions to set the compute unit price and limit.

#### Compute Unit Price
Wallets can monitor the average fee paid per compute unit for recently produced
blocks and suggest that users set a competitive compute unit price to add a
prioritization fee that will be partially paid to a Solana validator.

The RPC API doesn't provide an easy way to monitor fees for recently confirmed
transactions yet. More details will be shared soon but wallets could roll their
own approach by analyzing the transactions included in the stream of recently
confirmed blocks.

If the suggested prioritization fee is accepted by the user, the
`SetComputeUnitPrice` compute budget program instruction should be added to the
transaction's instruction list with the price denominated in micro-lamports.

<!-- TODO: web3.js example -->

### Set Compute Unit Limit
Wallets can simulate transactions to estimate how the transaction would be processed
if it were to be processed soon in a block.

The `simulateTransaction` RPC API doesn't return the total compute units
consumed by the transaction in the response (yet) but wallets can parse the
returned logs to get that information.

<!-- TODO: web3.js example -->
