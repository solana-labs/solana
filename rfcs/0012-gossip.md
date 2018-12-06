# Gossip, A.K.A. Cluster Info

This RFC describes the Solana's gossip protocol, also known as the network
control plane.

Nodes continuously share signed data objects among themselves in order to find
each other, structure the network data plane, prevent censoring, etc.

The basic mechanism is a 10Hz loop wherein every node sends a push message
and/or a pull message.  Push and pull messages may elicit responses, and push
messages may be forwarded on to others in the network.

Gossip runs on a well-known UDP/IP port, or a port in a well-known range.  Once
a network is bootstrapped, nodes advertise to each other where to find their
gossip endpoint (a socket address).

## Gossip Records

Records shared over gossip are arbitrary, but signed and versioned (with a
timestamp), and need to make sense to the node receiving them.  A strategy of
"most recent timestamp wins" is employed to manage what's stored in cluster
info.  If 2 values from the same creator have the same timestamp, the value
with the greater hash wins.

## Push

### Push Message

A push message is "hey network, I have something new to share".  Nodes send
push messages to `PUSH_FANOUT` push peers.

Upon receiving a push message, a node examines the message for:

1. duplication: if the message has been seen before, the node responds with
   `PushMessagePrune` and drops the message

2. new data: if the message is new to the node
    * stores the new information with an updated version in its cluster info and
       purges any previous older value
    * stores the message in `pushed_once` (used for detecting duplicates,
       purged after `PUSH_MSG_TIMEOUT * 5` ms)
    * retransmits the messages to its own push peers

3. expiration: nodes drop push messages that are older than `PUSH_MSG_TIMEOUT`

### Push Peers, Prune Message

A nodes selects its push peers at random from the active set of known peers.
The node keeps this selection for a relatively long time.  When a prune message
is received, the node drops the push peer that sent the prune.  Prune is an
indication that there is another, faster path to that node than direct push.

The set of push peers is kept fresh by rotating a new node into the set every
`PUSH_MSG_TIMEOUT/2` milliseconds.

## Pull

### Pull Message

A pull message is "hey dude, got anything new?".  A pull message is sent to a
single peer at random and comprises a Bloom filter that represents "things I
have or recently had".  A node receiving a pull message iterates over its values
and constructs a pull response of things that miss the filter and would fit in a
message.

A node constructs the pull Bloom filter by iterating over current values and
recently purged values.

A node handles items in a pull response the same way it handles new data in a
push message.


## Purging

Nodes retain prior versions of values (those updated by a pull or push) and
expired values (those older than `GOSSIP_PULL_CRDS_TIMEOUT_MS`) in
`purged_values` (things I recently had).  Nodes purge `purged_values` that are
older than `5 * GOSSIP_PULL_CRDS_TIMEOUT_MS`.
