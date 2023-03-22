# Bidirectional Quic Notification Service

## Problem

Currently, it is very hard to understand what happens to an individual transaction in
the cluster. As Solana is a distributed cluster, each transaction can be forwarded to
different nodes, may be rejected without any warning in the sig-verify stage, the node
would drop the connection, and much more. As a client, when I send a transaction to
the cluster, the only feedback I have is whether the transaction is in the blocks. This
feedback is insufficient when there is congestion in the network, and the transactions
are dropped without any information about where this process is happening and which part
of the code the cluster is struggling to keep up.

As there is no centralized logging system for Solana. When Solana mainnet goes under outage,
there is no easy way to debug what is happening except for relying on the logs of different
validators. That is why debugging Solana is a challenging feat. We want a client where we
can start listening to the notification for the banking stage for the upcoming leaders and
get information for the replay stage from other nodes. We want a service where we can subscribe
for notifications from a particular stage or service to understand how the cluster works.

As a client, I would like to know if the cluster rejects my transaction and the reason.
As a client, I would like to benchmark the testnet cluster to understand bottlenecks and
adapt my dapp design by following the lifecycle of a transaction. Solana logs are too verbose,
and we need access to all the machines in the cluster. So to get valid logs, we have to run a
validator and get a few slots as a leader. Solana metrics can be only used to get an overview
of statistics at a particular point in time but not a particular transaction.

Solana is being developed fast, and we have multiple development roadmaps to improve the
scheduling stage of the cluster. I propose adding a new notification mechanism to the Solana
cluster to understand more about the transaction lifecycle, replay errors, and consensus.
It will make developing, integrating, scaling, and understanding the Solana cluster easier.

## Solution

The new notification service should be fast with little overhead on the processing and network.
JSON RPC PUB SUB serialization and processing might have higher overhead, so we intend to use
QUIC bidirectional communication to make it lighter and minimize the impact on the network by
sending binary notification data of a fixed small size.

Solana currently uses QUIC to receive transactions from other validators or rpc nodes. Validator
usually listens in the cluster using a unidirectional channel, i.e., only clients can
send data to the cluster server without getting replies on the same channel. We will add an additional
service to have a minimum impact on the current design. A new quic bidirectional notification
service will run on a separate port to listen to bidirectional quic connection requests.
When a server gets a connection request on a bidirectional quic service, it will check if the
client is authorized to connect to this service via an Access Control List defined by identities
allowed while signing the connection or by some predefined limits. If the connection is authorized,
it will wait for a message from the client about the types of notifications the client is tracking.
A client can send either subscription or unsubscription request for notification.

```rust
#[derive(Clone, Serialize, Deserialize)]
pub enum NotificationType {
    NotifyTransaction {
        signature: Signature,
    },
    BankingStage,
    ReplayStage,
    ...
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SubscriptionType {
    subscription,
    unsubscription,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    notification_type: NotificationType,
    subscription_type: SubscriptionType,
}

```

Similar to logs, we will have a new macro, `notify` to send notifications. Notify
could be used to send notification messages for several `NotificationType`. All the clients
who have subscribed to the `NotificationType` will get the message. For example, when a transaction
fails, we can send notifications for NotifyTransaction and BankingStage. We encourage developers
to use notify in all the essential parts of the code.

Example:
```rust
if transaction.is_err() {
    notify!(transaction.err().to_string(),
        NotificationType::NotifyTransaction{signature},
        NotificationType::BankingStage,
    );
}
```

We can then create a new implementation of a connection pool where instead of just connecting to
the next N leaders, it will also connect to the bidirectional services of these leaders.
Before sending the transaction, it will subscribe to the `NotifyTransaction` notification
and then send the transaction to the leaders. Each leader will then reply about how they handled
the transactions, and we can easily determine the transaction's lifecycle.
We added a mechanism on the client side called a bidirectional reply handler which will take care
of the notifications received by each node client has subscribed to, and forward the notifications
to the output channel.

To avoid loops and over-notifications, validators will not create bidirectional channels with other
validators to get notifications. The size of the reply message will be a fixed length of N bytes
so that the client will know exactly that it has received a notification from the server after
receiving N bytes. The messages will be serialized in binary before sending over the network.
Here is the proposed structure:

```rust
pub const QUIC_REPLY_MESSAGE_SIZE: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
pub enum QuicNotification {
    TransactionMessage {
        sender_identity: Pubkey,
        transaction_signature: Signature,
        message: Vec<u8>, // Is resized to 128 chars in constructor
        approximate_slot: Slot,
        padding: [u8; ] // rest
    },
}
```

In the above enum, is the definition of `QuicNotification` for a transaction.
This enum will be serialized using `bincode` and sent over the network to the client.
The message length is exactly 128 chars. A serialized message for type `QuicNotification`
should have the exact length of `QUIC_NOTIFICATION_MESSAGE_SIZE`, which is set to 256 bytes at this time.
If the message is larger than 128 characters, it will be truncated; if it is smaller, the rest of
the characters will be set to `0`.

We can additionally add limits so that this feature could not be used as an attack vector.

1. Number of concurrent connections in bidirectional stream is limited to 5.
2. Additionally server will close the connection when client closes the connection.
3. Max number of subscriptions per connection : 10000
4. Number of messages in the queue to be sent limited to : 10000

We should also add a way to advertise the port for bidirectional message service. It could be fixed
to `QUIC TPU PORT + SOME PREDEFINED OFFSET`.

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