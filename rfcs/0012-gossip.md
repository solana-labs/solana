# Gossip, A.K.A. Cluster Info

This RFC describes the Solana's gossip protocol, also known as the network control plane.

Nodes continuously share signed data objects among themselves in order to find each other, structure the network data plane, prevent censoring, etc.

The basic mechanism is a 10Hz loop wherein every node sends a push message and/or a pull message.  Push and pull messages may elicit responses, and push messages may be forwarded on to others in the network.

Gossip runs on a well-known UDP/IP port, or a port in a well-known range.  Once a network is bootstrapped, nodes advertise to each other where to find their gossip endpoint (a socket address).

## Gossip Records

Records shared over gossip are arbitrary, but signed and versioned (with a timestamp), and need to make sense to the node receiving them.  A strategy of "most recently recevied wins" is employed to manage what's stored in cluster info.

## Push

### Push Message

A push message is "hey network, I have something new to share".  Nodes send push messages to a `fanout` number of push peers.

Upon receiving a push message, a node examines the message for:

1. duplication: if the message has been seen before, the node responds with a "prune" message drops the message.

2. new info: if the message is new the node
   a. stores the new information with an updated version in its cluster info
   b. stores the message in "pushed once" (used for detecting duplicates)
   c. retransmits the messages to its own push peers

3. timeout: a push message has a time to live, beyond which the message is dropped

### Push Peers, Prune Message

A nodes selects its push peers at random from the set of known peers.  The node keeps this selection for a relatively long time.  When a prune message is received, the node drops the push peer that sent the prune.  Prune is an indication that there is another, faster path to that node than direct push.

A new set of push peers is selected (TODO: when is that??).

### Other Stuff

A node holds messages for duplicate detection for `dup_timeout`, currently 5x a push message `timeout`.

## Pull

### Pull Message

A pull message is "hey dude, got anything new?".  A pull message is sent to a single peer at random and comprises a Bloom filter that represents "things I already have".  A node receiving a pull message iterates over its values and constructs a pull response of things that miss the filter and would fit in a message.

A node constructs the pull Bloom filter by iterating over the values it currently has.
