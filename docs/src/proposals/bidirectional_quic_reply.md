# Bidirectional Quic Reply

## Problem

Currently, it is very hard to understand what happens to an individual transaction in the cluster.
As Solana is a distributed cluster each transaction can be forwarded to different nodes, may be rejected
without any warning in the sig-verify stage, the connection could be dropped, and much more. As a client
when I send a transaction to the cluster the only feedback that I have is whether the transaction is
in the blocks or not. This feedback is insufficient when there is congestion in the network and the
transactions are dropped without any information about where this process is happening and which
part of the code the cluster is struggling to keep up.

As a client, I would like to know if my transaction is rejected by the cluster and what is the reason.
As a client, I would like to benchmark the testnet cluster to understand and adapt my dapp design by
following the lifecycle of a transaction. Solana logs are too verbose and we do not have access to
all the machines in the cluster. So to get valid logs we have to run a validator and get few slots
as a leader. Solana metrics can be used to just get an overview of statistics at a particular point
in time.

As Solana is being developed fast and we have multiple development roadmaps to improve the scheduling
stage of the cluster. I propose that we add a new notification mechanism to the Solana cluster
to understand more about the transaction lifecycle.
This will make it easier to develop, integrate, scale, and understand the Solana cluster.

## Solution

Solana currently uses QUIC to receive transactions from different clients either other validators or
rpc nodes. Validator usually listens in the cluster using a unidirectional channel, i.e only clients can
send data to the cluster server and cannot reply on the same channel. I propose that we also add a
possibility for the server to listen to a bidirectional quic connection requests. When a server gets a
request for this bidirectional quic channel it will get the transactions sent by the client using a
recv stream and the send stream will be used by a special class "bidirectional reply handler".

The bidirectional quic reply handler will then keep track of the transactions it receives from the IP
address which initialized the bidirectional channel. Similar to log messages we can emit notifications
for each transaction signature to the bidirectional reply handler which will then forward the
notification back to the client using send stream. The client has to just initialize a
bidirectional channel once and keep it alive to receive notifications about the transactions that it
has sent to the cluster. If the connection will be kept open by the server for a predetermined N
seconds and then will be closed. We can emit these notification at the most important stages of banking
stage and where transactions are probably lost.

A TPU client which connects to FANOUT_SLOTS/NUMBER_OF_SLOTS_PER_LEADER number of the next leader
then will initialize a bidirectional channel once with each of the leaders and send the transactions.
Each leader will then reply how it responded to the transactions and we can easily determine
the lifecycle of the transaction. We add a mechanism in the TPU client called a bidirectional
reply handler which will take care of the notifications received by each leader, and forward the
notifications to a crossbeam channel to be processed by the user of the TPU client.

To avoid loops and over-notifications, each bidirectional channel will be limited to a single validator,
the validator will not create bidirectional channels with other validators to get notifications.
The size of the reply message will be a fixed length of N bytes so that the client will exactly
know that it has received a notification from the server after receiving N bytes.
Here is the proposed structure:

```rust
pub const QUIC_REPLY_MESSAGE_SIZE: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
pub enum QuicReplyMessage {
    TransactionMessage {
        sender_identity: Pubkey,
        transaction_signature: Signature,
        message: Vec<u8>, // Is resized to 128 chars in constructor
        approximate_slot: Slot,
        padding: [u8; ] // rest
    },
}
```

In the above structure we have given an example for a QuicReplyMessage for a transaction, the message
length should be exactly 128 chars before serialization. A serialized message for type QuicReplyMessage
should have exact length of QUIC_REPLY_MESSAGE_SIZE which is set to be 256 bytes at this time. If the
message is larger than 128 characters then it will be truncated and if it is smaller then the rest of characters
will be set to `0`.

We can additionally add limits so that this feature could not be used as an attack vector.

1. Number of connections in bidirectional stream 5 unstaked and 5 staked.
2. Timeout for connection on server side : 30 seconds (i.e server will close connection in 30
seconds)
3. Additionally server will close the connection when client closes the connection.
4. Timeout for notification message on client side : 10s
5. Max number of transaction per connection : 10000
6. Number of messages in the queue to be sent limited to : 10000
7. Only staked identities can be used to create a bidirectional connections.
8. The minimum stake required is 10000 SOLs.

**This addition is over the unidirectional stream connection, so that it will support older clients too.**

## In working

We have already implemented this proposal for test purpose in v1.14.
<https://github.com/solana-labs/solana/pull/29954>

We have done some tests using mango bencher on a private cluster:

Example output:

Here we have multiple messages represented as ("message (slot)").

For successful transaction without congestion:
```
executed ([1147]), 
```

For successful transaction during congestion:
```
Transaction would exceed max Block Cost Limit ([1070]), 
Account in use (GUg52LVs8adjcdsGYYEwH5hGWfthgknQai8Pk4GQ2pWz) ([1070]), 
Account in use (3sWUkgYwyDLVnzLUGGX8Y33ph5iPG1WgUJWTpczf21QM) ([1070, 1071]),
executed ([1071]),
```

For silently failed transactions
(transaction was silently discarded as excess packet in sigverify stage) :
```
discarded as excess packet ([1110])
```