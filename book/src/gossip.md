# Gossip Service

The Gossip Service acts as a gateway to nodes in the control plane. Fullnodes
use the service to ensure information is available to all other nodes in a cluster.
The service broadcasts information using a gossip protocol.

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

## Gossip Service Interface

### Push Message

A node sends a push message to tells the cluster it has information to share.
Nodes send push messages to `PUSH_FANOUT` push peers.

Upon receiving a push message, a node examines the message for:

1. Duplication: if the message has been seen before, the node responds with
   `PushMessagePrune` and drops the message

2. New data: if the message is new to the node
   * Stores the new information with an updated version in its cluster info and
     purges any previous older value
   * Stores the message in `pushed_once` (used for detecting duplicates,
     purged after `PUSH_MSG_TIMEOUT * 5` ms)
   * Retransmits the messages to its own push peers

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

A node constructs the pull Bloom filter by iterating over current values and
recently purged values.

A node handles items in a pull response the same way it handles new data in a
push message.


## Purging

Nodes retain prior versions of values (those updated by a pull or push) and
expired values (those older than `GOSSIP_PULL_CRDS_TIMEOUT_MS`) in
`purged_values` (things I recently had).  Nodes purge `purged_values` that are
older than `5 * GOSSIP_PULL_CRDS_TIMEOUT_MS`.

## Eclipse Attacks

An eclipse attack is an attempt to take over the set of node connections with
adversarial endpoints.

This is relevant to our implementation in the following ways.

* Pull messages select a random node from the network.  An eclipse attack on
*pull* would require an attacker to influence the random selection in such a way
that only adversarial nodes are selected for pull.

* Push messages maintain an active set of nodes and select a random fanout for
every push message.  An eclipse attack on *push* would influence the active set
selection, or the random fanout selection.

### Time and Stake based weights

Weights are calculated based on `time since last picked` and the `natural log` of the `stake weight`.

Taking the `ln` of the stake weight allows giving all nodes a fairer chance of network
coverage in a reasonable amount of time. It helps normalize the large possible `stake weight` differences between nodes.
This way a node with low `stake weight`, compared to a node with large `stake weight` will only have to wait a
few multiples of ln(`stake`) seconds before it gets picked.

There is no way for an adversary to influence these parameters.

### Pull Message

A node is selected as a pull target based on the weights described above.

### Push Message

A prune message can only remove an adversary from a potential connection.

Just like *pull message*, nodes are selected into the active set based on weights.

## Notable differences from PlumTree

The active push protocol described here is based on [Plum
Tree](https://haslab.uminho.pt/jop/files/lpr07a.pdf).  The main differences are:

* Push messages have a wallclock that is signed by the originator.  Once the
wallclock expires the message is dropped.  A hop limit is difficult to implement
in an adversarial setting.

* Lazy Push is not implemented because its not obvious how to prevent an
adversary from forging the message fingerprint.  A naive approach would allow an
adversary to be prioritized for pull based on their input.
