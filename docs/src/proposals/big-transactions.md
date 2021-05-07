# Big Transactions

## Problem
Solana networking packets may not exceed the ipv6 MTU size to ensure fast and
reliable network transmission of cluster information over UDP. Solana's
networking stack uses a conservative MTU size of 1280 bytes which after
accounting for networking headers, leaves 1232 bytes for packet data like
serialized transactions.

Developers building applications on Solana have had to design their on-chain
programs in a way that works within the transaction size limit constraint. One
common work-around is to store temporary state on-chain and process the data in
later transactions. This is the approach used by the BPF loader program for
deploying Solana programs.

However, this workaround doesn't work well when users want to compose many
on-chain programs in a single atomic transaction because with more composition,
comes more account inputs. Each transaction account input takes up 32 bytes and
as programs get more complex, especially defi protocols, the number accounts
needed increases quite quickly. There is no available workaround for increasing
the number of accounts used in a single transaction, it's capped at less than 30
accounts after signatures and other metadata is accounted for.

## Proposed Solutions

1) Account input compression

Solana can provide a way to map commonly used addresses to small integer values.
This mapping can be computed on epoch boundaries by all validators or registered
on-chain in state managed by a new on-chain program.

This approach will require a new serialization format for compressed
transactions which can fit within a single network packet. Compressed
transactions will need to be identified as different from standard transactions
somehow. Either by a special encoding or using dedicated UDP ports for this new
type of transaction.

2) Transaction builder program

Solana can provide a new on-chain program which allows "Big" transactions to be
constructed on-chain by normal transactions. Once the transaction is
constructed, a final "Execute" transaction can trigger a node to process the big
transaction as a normal transaction without needing to fit it into an MTU sized
packet.