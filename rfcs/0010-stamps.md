# Stamps: Airdrops with Intent

The goal of this RFC is to define an airdrop mechanism that authorizes funds
for a particular use.

## Background

To run an on-chain program, a client must first acquire tokens to pay a Solana
cluster a transaction fee to instantiate the program. To acquire the tokens,
the client might request them from a special client called a *drone*.

## Problems

1. Once the client has tokens to instantiate the program, the client may choose
   to simply keep them.
2. Instantiating a program requires two transaction fees - one for the airdrop
   and one to instantiate the program.

## Stamps

Instead of a drone sending a payment transaction to the cluster, it could send
a signed *load* transaction back to the client. This signed transaction would
be called a stamp. Like how a food stamp may only be used to purchase food, a
drone's load stamp may only be used to load a program. It doesn't matter to the
cluster whether it receives a stamp from the client or the drone, only that the
signature is valid.

## Example usage

Creator of on-chain game tic-tac-toe hosts a drone that responds to airdrop
requests with an `InitGame` stamp. That stamp should pay the `InitGame`
transaction fee as well as seeding the game's account with enough tokens to
play it. Each time the user interacts with the game, the game pays the user
enough tokens to pay the next transaction fee to advance the game. At that
point, the user may choose to keep the tokens instead of advancing the game. If
the creator wants to defend against that case, they could require the user go
back to the drone for a new stamp after each move.
