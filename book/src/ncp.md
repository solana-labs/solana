# The Network Control Plane

The Network Control Plane (NCP) acts as a gateway to nodes in the control
plane. Fullnodes use the NCP to ensure information is available to all other
nodes in a cluster. The NCP broadcasts information using a gossip protocol.

## Gossip Overview

Nodes continuously share signed data objects among themselves in order to
manage a cluster. For example, they share their contact information, ledger
height, and votes.

Every tenth of a second, each node sends a "push" message and/or a "pull"
message.  Push and pull messages may elicit responses, and push messages may be
forwarded on to others in the cluster.

Gossip runs on a well-known UDP/IP port or a port in a well-known range.  Once
a cluster is bootstrapped, nodes advertise to each other where to find their
gossip endpoint (a socket address).

## Gossip Records

Records shared over gossip are arbitrary, but signed and versioned (with a
timestamp) as needed to make sense to the node receiving them. If a node
recieves two records from the same source, it it updates its own copy with the
record with the most recent timestamp.

## NCP Interface

### Push Message

A node sends a push message to tells the cluster it has information to share.
Nodes send push messages to `PUSH_FANOUT` push peers.

Upon receiving a push message, a node examines the message for:

1. Duplication: if the message has been seen before, the node responds with
   `PushMessagePrune` and drops the message

2. New information: if the message is new the node

   a. Stores the new information and updates its version

   b. Stores the message in `pushed_once` (used for detecting duplicates,
purged after `PUSH_MSG_TIMEOUT * 5` ms)

   c. Retransmits the messages to its own push peers

3. Expiration: nodes drop push messages that are older than `PUSH_MSG_TIMEOUT`

### Push Peers, Prune Message

A nodes selects its push peers at random from the active set of known peers.
The node keeps this selection for a relatively long time.  When a prune message
is received, the node drops the push peer that sent the prune.  Prune is an
indication that there is another, faster path to that node than direct push.

The set of push peers is kept fresh by rotating a new node into the set every
`PUSH_MSG_TIMEOUT/2` milliseconds.

### Pull Message

A node sends a pull message to ask the cluster if there is any new information.
A pull message is sent to a single peer at random and comprises a Bloom filter
that represents things it already has.  A node receiving a pull message
iterates over its values and constructs a pull response of things that miss the
filter and would fit in a message.

A node constructs the pull Bloom filter by iterating over the values it
currently has.

