# Multiple Gossip Identities

The current design of gossip requires that a single validator ID is run by a
single node. If more than one machine runs at the same time they will overwrite
each others CRDS values. Ideally, a validator might want to be able to run
multiple nodes in gossip that represent the same ID.

Some use cases:

- Enables H/A with multiple nodes to be run in parallel.
- Autoscaling by running low/high power nodes at the same time.
- Prioritize Turbine traffic to nodes shared by a single validator.



# Proposal

Extend gossip with another public key, known as a "node" key. This key only
identifies an individual machine in gossip, and is not associated with any on
chain state. `CrdsValue` entries are then associated with the node key instead
of the current ID key. The ID key can still sign and be verified against chain
state, so no security is lost.

Internally, lookups for the the validator ID can now treat gossip as "groups"
of nodes. Looking up some validator ID in gossip can return a list of
`ContactInfo` which for example Turbine can then use.



# Impact

1. Initially, this will mean turbine will begin sending data to multiple nodes
   owned by a single validator, which opens up a potential attack vector. To
   resolve this, Turbine should take into account validator groups and only
   send data to a single node. That node can then within its own turbine
   prioritize forwarding traffic to nodes in its own group. With this change
   this actually increases turbines throughput.

2. Internally, services should continue to use the ID key for packet and shred
   data, but many services rely on `cluster_info` and gossip to find this key.
   This change requires a careful check to make sure the correct keys are being
   used within the various staged.



# Alternatives

One alternative, is to allow VoteState to contain multiple ID keys. It already
allows multiple voting keys, though currently only one is possibly active as
the codebase currently handles it.

With this change, two `ID` keys in Gossip can be owned by the same VoteState. There
are a few problems with this model though:

1. VoteState key changes don't take effect until Epoch boundaries. Validators
   who are changing their internal machine setups likely don't have any effect
   on chain state, and having to wait for Epoch boundaries to introduce a new
   machine removes the possibility of things like fast autoscaling
2. The changes to implement this are more complex, as a lot of the codebase
   will have to deal with the possibility of receiving data signed by multiple
   different keys. A node key keeps the current one job one key status.
