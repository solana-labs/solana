# RiP Curl: low-latency, transaction-oriented RPC

## Problem

Solana's initial RPC implementation was created for the purpose of allowing
users to confirm transactions that had just recently been sent to the cluster.
It was designed with memory usage in mind such that any validator should be
able to support the API without concern of DoS attacks.

Later down the line, it became desirable to use that same API to support the
Solana explorer. The original design only supported minutes of history, so we
changed it to instead store transaction statuses in a local RocksDB instance
and offer days of history. We then extended that to 6 months via BigTable.

With each modification, the API became more suitable for applications serving
static content and less appealing for transaction processing. The clients poll
for transaction status instead of being notified, giving the false impression
of higher confirmation times. Furthermore, what clients can poll for is
limited, preventing them from making reasonable real-time decisions, such as
recognizing a transaction is confirmed as soon as particular, known
validators vote on it.

## Proposed Solution

A web-friendly, transaction-oriented, streaming API built around the
validator's ReplayStage.

Improved client experience:

- Support connections directly from WebAssembly apps.
- Clients can be notified of confirmation progress in real-time, including votes
  and voter stake weight.
- Clients can be notified when the heaviest fork changes, if it affects the
  transactions confirmation count.

Easier for validators to support:

- Each validator supports some number of concurrent connections and otherwise
  has no significant resource constraints.
- Transaction status is never stored in memory and cannot be polled for.
- Signatures are only stored in memory until the desired commitment level or
  until the blockhash expires, which ever is later.

How it works:

1. The client connects to a validator using a reliable communication channel,
   such as a web socket.
2. The validator registers the signature with ReplayStage.
3. The validator sends the transaction into the Gulf Stream and retries all
   known forks until the blockhash expires (not until the transaction is
   accepted on only the heaviest fork). If the blockhash expires, the
   signature is unregistered, the client is notified, and connection is closed.
4. As ReplayStage detects events affecting the transaction's status, it
   notifies the client in real-time.
5. After confirmation that the transaction is rooted (`CommitmentLevel::Max`),
   the signature is unregistered and the server closes the upstream channel.
