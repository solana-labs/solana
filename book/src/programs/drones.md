# Drones

This chapter defines an off-chain service called a _drone_, which acts as custodian of a user's private key. In its simplest form, it can be used to create _airdrop_ transactions, a token transfer from the drone's account to a client's account.

## Signing Service

A drone is a simple signing service. It listens for requests to sign _transaction data_. Once received, the drone validates the request however it sees fit. It may, for example, only accept transaction data with a `SystemInstruction::Transfer` instruction transferring only up to a certain amount of tokens. If the drone accepts the transaction, it returns an `Ok(Signature)` where `Signature` is a signature of the transaction data using the drone's private key. If it rejects the transaction data, it returns a `DroneError` describing why.

## Examples

### Granting access to an on-chain game

Creator of on-chain game tic-tac-toe hosts a drone that responds to airdrop requests containing an `InitGame` instruction. The drone signs the transaction data in the request and returns it, thereby authorizing its account to pay the transaction fee and as well as seeding the game's account with enough tokens to play it. The user then creates a transaction for its transaction data and the drones signature and submits it to the Solana cluster. Each time the user interacts with the game, the game pays the user enough tokens to pay the next transaction fee to advance the game. At that point, the user may choose to keep the tokens instead of advancing the game. If the creator wants to defend against that case, they could require the user to return to the drone to sign each instruction.

### Worldwide airdrop of a new token

Creator of a new on-chain token \(ERC-20 interface\), may wish to do a worldwide airdrop to distribute its tokens to millions of users over just a few seconds. That drone cannot spend resources interacting with the Solana cluster. Instead, the drone should only verify the client is unique and human, and then return the signature. It may also want to listen to the Solana cluster for recent entry IDs to support client retries and to ensure the airdrop is targeting the desired cluster.

Note: the Solana cluster will not parallelize transactions funded by the same fee-paying account. This means that the max throughput of a single fee-paying account is limited to the number of _ticks_ processed per second by the current leader. Add additional fee-paying accounts to improve throughput.

## Attack vectors

### Invalid recent\_blockhash

The drone may prefer its airdrops only target a particular Solana cluster. To do that, it listens to the cluster for new entry IDs and ensure any requests reference a recent one.

Note: to listen for new entry IDs assumes the drone is either a validator or a _light_ client. At the time of this writing, light clients have not been implemented and no proposal describes them. This document assumes one of the following approaches be taken:

1. Define and implement a light client
2. Embed a validator
3. Query the jsonrpc API for the latest last id at a rate slightly faster than

   ticks are produced.

### Double spends

A client may request multiple airdrops before the first has been submitted to the ledger. The client may do this maliciously or simply because it thinks the first request was dropped. The drone should not simply query the cluster to ensure the client has not already received an airdrop. Instead, it should use `recent_blockhash` to ensure the previous request is expired before signing another. Note that the Solana cluster will reject any transaction with a `recent_blockhash` beyond a certain _age_.

### Denial of Service

If the transaction data size is smaller than the size of the returned signature \(or descriptive error\), a single client can flood the network. Considering that a simple `Transfer` operation requires two public keys \(each 32 bytes\) and a `fee` field, and that the returned signature is 64 bytes \(and a byte to indicate `Ok`\), consideration for this attack may not be required.

In the current design, the drone accepts TCP connections. This allows clients to DoS the service by simply opening lots of idle connections. Switching to UDP may be preferred. The transaction data will be smaller than a UDP packet since the transaction sent to the Solana cluster is already pinned to using UDP.

