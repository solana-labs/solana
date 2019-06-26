# token-exchange
Solana Token Exchange Bench

If you can't wait; jump to [Running the exchange](#Running-the-exchange) to
learn how to start and interact with the exchange.

### Table of Contents
[Overview](#Overview)<br>
[Premiss](#Premiss)<br>
[Exchange startup](#Exchange-startup)<br>
[Order requests](#Order-requests)<br>
[Order cancellations](#Order-cancellations)<br>
[Trades](#trade)<br>
[Exchange program operations](#Exchange-program-operations)<br>
[Quotes and OHLCV](#Quotes-and-OHLCV)<br>
[Investor strategies](#Investor-strategies)<br>
[Running the exchange](#Running-the-exchange)<br>

## Overview

An exchange is a marketplace where one asset can be traded for another.  This
demo demonstrates one way to host an exchange on the Solana blockchain by
emulating a currency exchange.

The assets are virtual tokens held by investors who may post order requests to
the exchange.  A Matcher monitors the exchange and issues match requests for
matching trade orders.  All the transactions can execute concurrently.

## Premise

- Exchange
  -  An exchange is a marketplace where one asset can be traded for another.
     The exchange in this demo is the on-chain program that implements the
     tokens and the policies for trading those tokens.
- Token
  - A virtual asset that can be owned, traded, and holds virtual intrinsic value
    compared to other assets.  There are four types of tokens in this demo, A,
    B, C, D.  Each one may be traded for another.
- Exchange account
  - An account maintained by the exchange that holds a quantity of one type of asset.
- Account request
  - A request to create an exchange account
- Token request
  - A request to deposit tokens of a particular type into an exchange account.
- Price ratio
  -  An expression of the relative prices of two tokens.  They consist of the
     price of the primary token and the price of the secondary token.  For
     simplicity sake, the primary token's price is always 1, which forces the
     secondary to be the common denominator.  For example, if token A was worth
     2 and token B was worth 6, the price ratio would be 1:3 or just 3.  Price
     ratios are represented as fixed point numbers.  The fixed point scaler is
     defined in
     [exchange_state.rs](https://github.com/solana-labs/solana/blob/c2fdd1362a029dcf89c8907c562d2079d977df11/programs/exchange_api/src/exchange_state.rs#L7)
- Order Requests
  - A Solana transaction sent by the owner of an exchange account and executed
    by the exchange, resulting in the creation of an Order. It has these components:
    - Offer - the contract address of an asset and an amount of the asset for sale by the maker
    - Accept - the contract address of a asset and an amount of the asset accepted as compensation by the maker
    - Expiration - a unix timestamp denoting the time of expiration of the order
    - OrderOptions - optional parameters for trade execution/settlement behavior
- Offer/Accept
  - Are used to specify the terms of an Order. The order issuer elects to offer
    certain assetAmounts and to accept as others as payment
  - An AssetAmount (improve name) consists of an asset as identified by its contract
    address and an amount of that asset in base units.
  - The Offer and Accept fields should be vectors populated by between 1 and n AssetAmounts
    where n is a small number (3-5).
  - for efficient handling and execution, should be limited to 1:1, 1:n, or n:1, not n:n
  - The point of these multi-asset orders is to improve the marginal utility of capital
    for traders in a way that is intrinsic to the exchange and does not require rebates
    or grant programs to incentivize market makers. They are not an MVP feature.
- Orders
  - The result of a successful order request.  Orders are stored in
    accounts owned by the submitter of the order request.  They can only be
    canceled by their owner but can be used by anyone in a match.  They
    contain the same information as the order request.
  - Multi-asset Orders are orders which have more than one asset amount in either
    the offer or accept field

- Price spread
  - The difference between the two matching orders. The spread is the
    profit of the matcher initiating the swap request.
- Execution Conditions
  - Conditions which when met, result in a successful trade being carried out.
    - Orders in question must complement each other in offer/accept asset directionality
    - Orders must be valid and active (not expired/cancelled)
    - Orders must be issued by different accounts (self-trade prevention)
- Match request
  - A request submitted to the exchange to carry out the matching process
  - Requires a set of valid, active orders that meet execution conditions
- Match
  - Initiated via the sending of a match request to the exchange, a basic match
    requires two valid, complementary orders and if successful, results in a Trade being produced.
  - Requested by a 3rd party matcher under normal circumstances, but may be carried out
    by a direct stakeholder in the transaction
  - Pays out a sum equal to the price spread of the orders times the volume
    matched to the initiator of the Match Request
- Trade
  - A successful trade is carried out as a result of matching two valid orders that meet
    execution conditions.  A trade may not wholly satisfy one or both of the
    orders in which case the affected orders are adjusted appropriately.  As
    long as the execution requirements are met there will be an exchange of tokens
    between accounts. Trades are recorded in a new account for posterity.
- Investor
  - Individual investors who hold a number of tokens and wish to trade them on
    the exchange.  Investors operate as Solana clients who own a set of
    exchange accounts containing tokens and/or orders.  Investors make
    transactions to the exchange in order to request tokens and instantiate or cancel
    orders.
- Matcher
  - An agent who facilitates trading between investors.  Matchers operate as
    Solana clients who monitor the pool of available orders looking for matchable
    order pairs. Once found, the matcher issues a match request to the exchange.
    Matchers are the engine of the exchange and are rewarded for their efforts by
    accumulating the price spreads of the swaps they initiate.
- Transaction fees
  - Solana transaction fees are paid for by the transaction submitters who are
    the Investors and Matchers.
- Market Validator
  - Solana network validators which have elected to compute and track market
    state and publish event feeds and other data with public API's.
    <!-- There needs to be some kind of incentive to drive this behavior in the long term,
    so setting it up as a marketplace for data where suppliers compete in a largely
    fungible product with a low barrier to entry seems like a good way to align
    incentives toward stable, honest actors.  -->

## Exchange startup

The exchange is up and running when it reaches a state where it can take
investor's trades and Matcher's swap requests.  To achieve this state the
following must occur in order:

- Start the Solana blockchain
- Start the Matcher thin-client
- The Matcher subscribes to change notifications for all the accounts owned by
  the exchange program id.  The subscription is managed via Solana's JSON RPC
  interface.
- Matchers, investors, and administrators of exchange frontends query Market Validator
  API for market state and event feeds and refer to replicators for historic data


The Matcher responding successfully to price and OHLCV requests is the signal to
the investors that trades submitted after that point will be analyzed.  <!--This
is not ideal, and instead investors should be able to submit trades at any time,
and the Matcher could come and go without missing a trade.  One way to achieve
this is for the Matcher to read the current state of all accounts looking for all
open trade orders.-->

Investors will initially query the exchange to discover their current balance
for each type of token.  If the investor does not already have an account for
each type of token, they will submit account requests.  Matchers as well will
request accounts to hold the tokens they earn by initiating trades.

```rust
/// Supported token types
pub enum Token {
    A,
    B,
    C,
    D,
}

pub enum ExchangeInstruction {
    /// New token account
    /// key 0 - Signer
    /// key 1 - New token account
    AccountRequest,
}

/// Token accounts are populated with this structure
pub struct TokenAccountInfo {
    /// Investor who owns this account
    pub owner: Pubkey,
    /// Current number of tokens this account holds
    pub tokens: Tokens,
}
```

For this demo investors or Matchers can request more tokens from the exchange at
any time by submitting token requests. In non-demos, an exchange of this type
would provide another way to exchange a 3rd party asset into tokens.

To request tokens, investors submit transfer requests:

```rust
pub enum ExchangeInstruction {
    /// Transfer tokens between two accounts
    /// key 0 - Account to transfer tokens to
    /// key 1 - Account to transfer tokens from.  This can be the exchange program itself,
    ///         the exchange has a limitless number of tokens it can transfer.
    TransferRequest(Token, u64),
}
```

## Order requests

When an investor decides to exchange a token of one type for another, they
submit a transaction to the Solana Blockchain containing an order request, which,
if successfully validated, is turned into an order. Active orders can be cancelled
by sending a cancel request to the exchange and have an optional expiration field,
which takes a timestamp after which the order is considered invalid.
When an order is created, tokens are deducted from a token account and the
order acts as an escrow.  The tokens are held until the trade order is filled,
canceled, or expired.

In the event that the order is matched and fully executes, one or more AssetAmounts
from the offer field will reflect the amount and identity of assets deducted from
the primary account and credited to the secondary account. orders are
no longer valid when the number of `tokens` goes to zero, at which point they
can no longer be used.
<!-- Could support refilling trade orders, so trade order
accounts are refilled rather than accumulating -->



```rust
/// holds asset/amount pairs
pub struct AssetAmount { /// needs better name (AssetPool, AssetInfo ?)
    /// Asset in question, as identified by contract address
    pub Asset: Pubkey,
    /// Amount of the Asset in question in base units
    pub Amount: u64,
}

pub enum SettlementType {
    Auto,     // Automatically settle entire order to underlying asset
    Deferred, // Transfer tokens but don't begin settlement process
    Partial,  // Settle a fraction of the order to underlying asset (not mvp feature)
}

<!-- not mvp feature -->
pub enum OrderType {
    Limit,
    Market,
    Stop,
}

/// holds options for order execution and handling
pub struct OrderOptions {
    // settlement account is the account to settle to (if compound transaction)
    pub SettlementAccount: Option<Pubkey>,

    // style of settlement behavior
    pub SettlementType: Option<SettlementType>,

    // Time after which order is no longer valid
    pub ExpirationTs: Option<u32>,
}

pub struct OrderRequestInfo {

    /// Asset(s) to offer in the trade
    pub Offer: Vec<AssetAmount>,

    // Asset(s) accepted as payment
    pub Accept: Vec<AssetAmount>,

    /// Order parameters and execution options
    pub Options: OrderOptions,

    /// Solana account to deposit assets on successful trade
    pub DepositAccount: Pubkey,
}

pub enum ExchangeInstruction {
    /// Order request
    /// key 0 - Signer
    /// key 1 - Account in which to record the trade
    /// key 2 - Exchange account associated with this order
    OrderRequest(OrderRequestInfo),
}

/// Trade accounts are populated with this structure
pub struct OrderInfo {
    /// Owner of the trade order
    pub owner: Pubkey,

    pub Offer: Vec<AssetAmount>,

    pub Accept: Vec<AssetAmount>,

    /// account which the tokens were source from.  The trade account holds the tokens in escrow
    /// until either one or more part of a swap or the trade is canceled.
    pub SourceAccount: Pubkey,
    /// account which the tokens the tokens will be deposited into on a successful trade
    pub DepositAccount: Pubkey,
}
```

## Order cancellations

An investor may cancel a trade at anytime, but only trades they own.  If the
cancellation is successful, any tokens held in escrow are returned to the
account from which they came.

```rust
pub enum ExchangeInstruction {
    /// Order cancellation
    /// key 0 - Signer
    /// key 1 - Order to cancel
    OrderCancellation,
}
```

## Trades

The Matcher is monitoring the accounts assigned to the exchange program and
building a trade-order table.  The trade order table is used to identify
matching trade orders which could be fulfilled.  When a match is found the
Matcher should issue a match request.  Match requests may not satisfy the entirety
of either order, but the exchange will greedily fulfill it.  Any leftover tokens
in either account will keep the Order valid for further matches in
the future.

Matching trade orders are defined by the following swap requirements:

- Opposite polarity offerA coincides with acceptB and acceptA with offerB
- Operate on the same assets
- There are sufficient tokens to perform the trade

Orders can be written in the following format:

`investor offerAsset offerAmount acceptAsset acceptAmount `

For example:

- `Ix A 2 B 2`
  - Investor x wishes to exchange 2 A tokens to B tokens at a ratio of 1 A to 1
    B
- `Iy A 5 B 6`
  - Investor y wishes to exchange 5 A tokens for 6 B tokens at a ratio of 1A:1.2B

A Matcher would likely maintain a table of outstanding valid orders for a given marketplace
Take the market representation A/B the table would look something like this

  "Offer A / Accept B" (asks)       "Offer B / Accept A" (bids)
|       Ix A 1 B 2            | 1 |         Iy B 3 A 1.25            |
|       Im A 2 B 4.2          | 2 |         Iz B 5 A 3               |
|       In A 1 B 3            | 3 |         It B 3 A 2               |
|       Io A 4 B 15           | 4 |         Is B 2 A 2               |

In this example, the matcher has detected the above transactions and ordered them in
two arrays by computed price ratio with the asks descending and bids ascending.
Here, the first action by the matcher is to issue a match request using bid1 and ask1,
resulting in a Trade being generated leading to the following balance adjustments:

           A      B
Ix        -1      +2
Iy        +1      -2.4
Matcher    0      +0.4

This therefore results in the following revised order table, where the original ask1 has
been completely filled and removed and bid1 has been updated to reflect its remaining
funds.

"Offer A / Accept B" (asks)       "Offer B / Accept A" (bids)
|       Im A 2 B 4.2        | 1 |         Iy B .6 A .25            |
|       Ib A 1 B 3          | 2 |         Iz B 5 A 3               |
|       Ic A 4 B 15         | 3 |         Ix B 3 A 2               |
|                           | 4 |         Ix B 2 A 2               |

Since the new ask1 and bid1 price ratios are .476 and .416 (ie. Investor m offers
to sell A at a price of .476:1 and Investor y offers to buy A at .416:1 ), no new
action will take place at this stage until such time as more potentially matchable
orders are submitted.


```rust
pub enum ExchangeInstruction {
    /// match request
    /// key 0 - Signer
    /// key 1 - Account in which to record the trade
    /// key 2 - Order A (Order polarity makes no difference to execution)
    /// key 3 - Order B
    /// key 4 - Token account associated with the A Order
    /// key 5 - Token account associated with the B Order
    /// key 6 - Token account in which to deposit the Matchers profit from the swap.
    MatchRequest,
}

/// Trade accounts are populated with this structure
pub struct TradeInfo {
    pub Offered: Vec<AssetAmount>,
    /// Assets offered for trade
    pub Accepted: Vec<AssetAmount>,
    /// Assets accepted for trades
    /// Offer/Accept for trade have arbitrary "polarity" eg. swapping them in the trade context should have no effect on logic.
    // Might be better to name them in a way that doesn't imply temporal relationship

    pub OfferedOrder: Pubkey,
    /// order that supplied the assets referred in the Offered field
    pub AcceptedOrder: Pubkey,
    /// order that supplied the assets referred in the Accepted field
    pub Timestamp: u32,
}
```

## Exchange program operations

Putting all the commands together from above, the following operations will be
supported by the on-chain exchange program:

```rust
pub enum ExchangeInstruction {
    /// New token account
    /// key 0 - Signer
    /// key 1 - New token account
    AccountRequest,

    /// Transfer tokens between two accounts
    /// key 0 - Account to transfer tokens to
    /// key 1 - Account to transfer tokens from.  This can be the exchange program itself,
    ///         the exchange has a limitless number of tokens it can transfer.
    TransferRequest(Token, u64),

    /// Order request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - Token account associated with this order
    OrderRequest(OrderRequestInfo),

    /// Order cancellation
    /// key 0 - Signer
    /// key 1 - Order to cancel
    OrderCancellation,

    /// match request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - A order
    /// key 3 - B order
    /// key 4 - Token account associated with the A order
    /// key 5 - Token account associated with the B order
    /// key 6 - Token account in which to deposit the Matcher's profit from the match.
    MatchRequest,
}
```

## Quotes and OHLCV

The Matcher will provide current bid/ask price quotes based on trade actively and
also provide OHLCV based on some time window.  The details of how the bid/ask
price quotes are calculated are yet to be decided.

## Investor strategies

To make a compelling demo, the investors needs to provide interesting trade
behavior.  Something as simple as a randomly twiddled baseline would be a
minimum starting point.

## Running the exchange

The exchange bench posts trades and swaps matches as fast as it can.  

You might want to bump the duration up
to 60 seconds and the batch size to 1000 for better numbers.  You can modify those
in client_demo/src/demo.rs::test_exchange_local_cluster.

The following command runs the bench:

```bash
$ RUST_LOG=solana_bench_exchange=info cargo test --release -- --nocapture test_exchange_local_cluster
```

To also see the cluster messages:

```bash
$ RUST_LOG=solana_bench_exchange=info,solana=info cargo test --release -- --nocapture test_exchange_local_cluster
```
