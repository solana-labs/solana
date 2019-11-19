# token-exchange
Solana Token Exchange Bench

If you can't wait; jump to [Running the exchange](#Running-the-exchange) to
learn how to start and interact with the exchange.

### Table of Contents
[Overview](#Overview)<br>
[Premise](#Premise)<br>
[Exchange startup](#Exchange-startup)<br>
[Order Requests](#Trade-requests)<br>
[Order Cancellations](#Trade-cancellations)<br>
[Trade swap](#Trade-swap)<br>
[Exchange program operations](#Exchange-program-operations)<br>
[Quotes and OHLCV](#Quotes-and-OHLCV)<br>
[Investor strategies](#Investor-strategies)<br>
[Running the exchange](#Running-the-exchange)<br>

## Overview

An exchange is a marketplace where one asset can be traded for another.  This
demo demonstrates one way to host an exchange on the Solana blockchain by
emulating a currency exchange.

The assets are virtual tokens held by investors who may post order requests to
the exchange.  A Matcher monitors the exchange and posts swap requests for
matching orders.  All the transactions can execute concurrently.

## Premise

- Exchange
  -  An exchange is a marketplace where one asset can be traded for another.
     The exchange in this demo is the on-chain program that implements the
     tokens and the policies for trading those tokens.
- Token
  - A virtual asset that can be owned, traded, and holds virtual intrinsic value
    compared to other assets.  There are four types of tokens in this demo, A,
    B, C, D.  Each one may be traded for another.
- Token account
  - An account owned by the exchange that holds a quantity of one type of token.
- Account request
  - A request to create a token account
- Token request
  - A request to deposit tokens of a particular type into a token account.
- Asset pair
  - A struct with fields Base and Quote, representing the two assets which make up a 
    trading pair,  which themselves are Tokens. The Base or 'primary' asset is the
    numerator and the Quote is the denominator for pricing purposes.
- Order side
  - Describes which side of the market an investor wants to place a trade on. Options
    are "Bid" or "Ask", where a bid represents an offer to purchase the Base asset of
    the AssetPair for a sum of the Quote Asset and an Ask is an offer to sell Base asset
    for the Quote asset.
- Price ratio
  -  An expression of the relative prices of two tokens. Calculated with the Base
     Asset as the numerator and the Quote Asset as the denominator. Ratios are 
     represented as fixed point numbers.  The fixed point scaler is defined in
     [exchange_state.rs](https://github.com/solana-labs/solana/blob/c2fdd1362a029dcf89c8907c562d2079d977df11/programs/exchange_api/src/exchange_state.rs#L7)
- Order request
  - A Solana transaction sent by a trader to the exchange to submit an order. 
    Order requests are made up of the token pair, the order side (bid or ask),
    quantity of the primary token, the price ratio, and the two token accounts
    to be credited/deducted.  An example trade request looks like "T AB 5 2" 
    which reads "Exchange 5 A tokens to B tokens at a price ratio of 1:2"  A fulfilled trade would result in 5 A tokens
    deducted and 10 B tokens credited to the trade initiator's token accounts.
    Successful order requests result in an order.
- Order
  - The result of a successful order request.  orders are stored in
    accounts owned by the submitter of the order request.  They can only be
    canceled by their owner but can be used by anyone in a trade swap.  They
    contain the same information as the order request.
- Price spread
  - The difference between the two matching orders. The spread is the
    profit of the Matcher initiating the swap request.
- Match requirements
  - Policies that result in a successful trade swap.
- Match request
  - A request to fill two complementary orders (bid/ask), resulting if successful,
    in a trade being created.
- Trade
  - A successful trade is created from two matching orders that meet
    swap requirements which are submitted in a Match Request by a Matcher and
    executed by the exchange. A trade may not wholly satisfy one or both of the
    orders in which case the orders are adjusted appropriately. Upon execution,
    tokens are distributed to the traders' accounts and any overlap or 
    "negative spread" between orders is deposited into the Matcher's profit
    account.  All successful trades are recorded in the data of a new solana 
    account for posterity.
- Investor
  - Individual investors who hold a number of tokens and wish to trade them on
    the exchange.  Investors operate as Solana thin clients who own a set of
    accounts containing tokens and/or order requests.  Investors post
    transactions to the exchange in order to request tokens and post or cancel
    order requests.
- Matcher
  - An agent who facilitates trading between investors.  Matchers operate as
    Solana thin clients who monitor all the orders looking for a trade
    match.  Once found, the Matcher issues a swap request to the exchange.
    Matchers are the engine of the exchange and are rewarded for their efforts by
    accumulating the price spreads of the swaps they initiate.  Matchers also
    provide current bid/ask price and OHLCV (Open, High, Low, Close, Volume)
    information on demand via a public network port.
- Transaction fees
  - Solana transaction fees are paid for by the transaction submitters who are
    the Investors and Matchers.

## Exchange startup

The exchange is up and running when it reaches a state where it can take
investors' trades and Matchers' match requests.  To achieve this state the
following must occur in order:

- Start the Solana blockchain
- Start the  thin-client
- The Matcher subscribes to change notifications for all the accounts owned by
  the exchange program id.  The subscription is managed via Solana's JSON RPC
  interface.
- The Matcher starts responding to queries for bid/ask price and OHLCV

The Matcher responding successfully to price and OHLCV requests is the signal to
the investors that trades submitted after that point will be analyzed.  <!--This
is not ideal, and instead investors should be able to submit trades at any time,
and the Matcher could come and go without missing a trade.  One way to achieve
this is for the Matcher to read the current state of all accounts looking for all
open orders.-->

Investors will initially query the exchange to discover their current balance
for each type of token.  If the investor does not already have an account for
each type of token, they will submit account requests.  Matcher as well will
request accounts to hold the tokens they earn by initiating trade swaps.

```rust
/// Supported token types
pub enum Token {
    A,
    B,
    C,
    D,
}

/// Supported token pairs
pub enum TokenPair {
    AB,
    AC,
    AD,
    BC,
    BD,
    CD,
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

For this demo investors or Matcher can request more tokens from the exchange at
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

## Order Requests

When an investor decides to exchange a token of one type for another, they
submit a transaction to the Solana Blockchain containing an order request, which,
if successful, is turned into an order.  orders do not expire but are
cancellable. <!-- orders should have a timestamp to enable trade
expiration -->  When an order is created, tokens are deducted from a token
account and the order acts as an escrow.  The tokens are held until the
order is fulfilled or canceled. If the direction is `To`, then the number
of `tokens` are deducted from the primary account, if `From` then `tokens`
multiplied by `price` are deducted from the secondary account.  orders are
no longer valid when the number of `tokens` goes to zero, at which point they
can no longer be used. <!-- Could support refilling orders, so order
accounts are refilled rather than accumulating -->

```rust
/// Direction of the exchange between two tokens in a pair
pub enum Direction {
    /// Trade first token type (primary) in the pair 'To' the second
    To,
    /// Trade first token type in the pair 'From' the second (secondary)
    From,
}

pub struct OrderRequestInfo {
    /// Direction of trade
    pub direction: Direction,

    /// Token pair to trade
    pub pair: TokenPair,

    /// Number of tokens to exchange; refers to the primary or the secondary depending on the direction
    pub tokens: u64,

    /// The price ratio the primary price over the secondary price.  The primary price is fixed
    /// and equal to the variable `SCALER`.
    pub price: u64,

    /// Token account to deposit tokens on successful swap
    pub dst_account: Pubkey,
}

pub enum ExchangeInstruction {
    /// order request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - Token account associated with this trade
    TradeRequest(TradeRequestInfo),
}

/// Trade accounts are populated with this structure
pub struct TradeOrderInfo {
    /// Owner of the order
    pub owner: Pubkey,
    /// Direction of the exchange
    pub direction: Direction,
    /// Token pair indicating two tokens to exchange, first is primary
    pub pair: TokenPair,
    /// Number of tokens to exchange; primary or secondary depending on direction
    pub tokens: u64,
    /// Scaled price of the secondary token given the primary is equal to the scale value
    /// If scale is 1 and price is 2 then ratio is 1:2 or 1 primary token for 2 secondary tokens
    pub price: u64,
    /// account which the tokens were source from.  The trade account holds the tokens in escrow
    /// until either one or more part of a swap or the trade is canceled.
    pub src_account: Pubkey,
    /// account which the tokens the tokens will be deposited into on a successful trade
    pub dst_account: Pubkey,
}
```

## Order cancellations

An investor may cancel a trade at anytime, but only trades they own.  If the
cancellation is successful, any tokens held in escrow are returned to the
account from which they came.

```rust
pub enum ExchangeInstruction {
    /// order cancellation
    /// key 0 - Signer
    /// key 1 -order to cancel
    TradeCancellation,
}
```

## Trade swaps

The Matcher is monitoring the accounts assigned to the exchange program and
building a trade-order table.  The order table is used to identify
matching orders which could be fulfilled.  When a match is found the
Matcher should issue a swap request.  Swap requests may not satisfy the entirety
of either order, but the exchange will greedily fulfill it.  Any leftover tokens
in either account will keep the order valid for further swap requests in
the future.

Matching orders are defined by the following swap requirements:

- Opposite polarity (one `To` and one `From`)
- Operate on the same token pair
- The price ratio of the `From` order is greater than or equal to the `To` order
- There are sufficient tokens to perform the trade

Orders can be written in the following format:

`investor direction pair quantity price-ratio`

For example:

- `1 T AB 2 1`
  - Investor 1 wishes to exchange 2 A tokens to B tokens at a ratio of 1 A to 1
    B
- `2 F AC 6 1.2`
  - Investor 2 wishes to exchange A tokens from 6 B tokens at a ratio of 1 A
    from 1.2 B

An order table could look something like the following. Notice how the columns
are sorted low to high and high to low, respectively.  Prices are dramatic and
whole for clarity.

|Row| To          | From       |
|---|-------------|------------|
| 1 | 1 T AB 2 4  | 2 F AB 2 8 |
| 2 | 1 T AB 1 4  | 2 F AB 2 8 |
| 3 | 1 T AB 6 6  | 2 F AB 2 7 |
| 4 | 1 T AB 2 8  | 2 F AB 3 6 |
| 5 | 1 T AB 2 10 | 2 F AB 1 5 |

As part of a successful swap request, the exchange will credit tokens to the
Matcher's account equal to the difference in the price ratios or the two orders.
These tokens are considered the Matcher's profit for initiating the trade.

The Matcher would initiate the following swap on the order table above:

  - Row 1, To:   Investor 1 trades 2 A tokens to 8 B tokens
  - Row 1, From: Investor 2 trades 2 A tokens from 8 B tokens
  - Matcher takes 8 B tokens as profit

Both row 1 trades are fully realized, table becomes:

|Row| To          | From       |
|---|-------------|------------|
| 1 | 1 T AB 1 4  | 2 F AB 2 8 |
| 2 | 1 T AB 6 6  | 2 F AB 2 7 |
| 3 | 1 T AB 2 8  | 2 F AB 3 6 |
| 4 | 1 T AB 2 10 | 2 F AB 1 5 |

The Matcher would initiate the following swap:

  - Row 1, To:   Investor 1 trades 1 A token to 4 B tokens
  - Row 1, From: Investor 2 trades 1 A token from 4 B tokens
  - Matcher takes 4 B tokens as profit

Row 1 From is not fully realized, table becomes:

|Row| To          | From       |
|---|-------------|------------|
| 1 | 1 T AB 6 6  | 2 F AB 1 8 |
| 2 | 1 T AB 2 8  | 2 F AB 2 7 |
| 3 | 1 T AB 2 10 | 2 F AB 3 6 |
| 4 |             | 2 F AB 1 5 |

The Matcher would initiate the following swap:

  - Row 1, To:   Investor 1 trades 1 A token to 6 B tokens
  - Row 1, From: Investor 2 trades 1 A token from 6 B tokens
  - Matcher takes 2 B tokens as profit

Row 1 To is now fully realized, table becomes:

|Row| To          | From       |
|---|-------------|------------|
| 1 | 1 T AB 5 6  | 2 F AB 2 7 |
| 2 | 1 T AB 2 8  | 2 F AB 3 5 |
| 3 | 1 T AB 2 10 | 2 F AB 1 5 |

The Matcher would initiate the following last swap:

  - Row 1, To:   Investor 1 trades 2 A token to 12 B tokens
  - Row 1, From: Investor 2 trades 2 A token from 12 B tokens
  - Matcher takes 2 B tokens as profit

Table becomes:

|Row| To          | From       |
|---|-------------|------------|
| 1 | 1 T AB 3 6  | 2 F AB 3 5 |
| 2 | 1 T AB 2 8  | 2 F AB 1 5 |
| 3 | 1 T AB 2 10 |            |

At this point the lowest To's price is larger than the largest From's price so
no more swaps would be initiated until new orders came in.

```rust
pub enum ExchangeInstruction {
    /// Trade swap request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - 'To' order
    /// key 3 - `From` order
    /// key 4 - Token account associated with the To Trade
    /// key 5 - Token account associated with From trade
    /// key 6 - Token account in which to deposit the Matcher profit from the swap.
    SwapRequest,
}

/// Swap accounts are populated with this structure
pub struct TradeSwapInfo {
    /// Pair swapped
    pub pair: TokenPair,
    /// `To` order
    pub to_trade_order: Pubkey,
    /// `From` order
    pub from_trade_order: Pubkey,
    /// Number of primary tokens exchanged
    pub primary_tokens: u64,
    /// Price the primary tokens were exchanged for
    pub primary_price: u64,
    /// Number of secondary tokens exchanged
    pub secondary_tokens: u64,
    /// Price the secondary tokens were exchanged for
    pub secondary_price: u64,
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

    /// order request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - Token account associated with this trade
    TradeRequest(TradeRequestInfo),

    /// order cancellation
    /// key 0 - Signer
    /// key 1 -order to cancel
    TradeCancellation,

    /// Trade swap request
    /// key 0 - Signer
    /// key 1 - Account in which to record the swap
    /// key 2 - 'To' order
    /// key 3 - `From` order
    /// key 4 - Token account associated with the To Trade
    /// key 5 - Token account associated with From trade
    /// key 6 - Token account in which to deposit the Matcher profit from the swap.
    SwapRequest,
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
