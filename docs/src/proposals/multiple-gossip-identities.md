# Multiple Gossip Identities

The current design of gossip identifies a validator by an identity key. This
means a validator that wishes to run more than one node sharing that key cannot
have each machine uniquely appear in gossip without trampling over each others
gossip data.

Validators might actually want to be able to run multiple nodes that share an
identity however it enables H/A style setups to work correctly, in particular
allowing autoscaling and shared KMS setups.



# Overview of Required Changes

## Addition of a Node Key

For a validator to share state among machines, it needs some way of being
grouped. One way to do this would be to modify the `VoteState` to allow for
multiple ID keys to sign. This however has a large flaw.

A validator must  wait for the chain to confirm ID key changes which forces
their node topology to be dictated by consensus. A validator's topology however
might be driven by something like H/A, which must assume the chain has lost
consistency in which case consensus may never be achieved.

This proposal suggests to enable this by istead adding a second key to gossip
identities to allow individual machines owned by a single validator to be
uniquely identified. A validators unique key then becomes a pair of keys: `(ID, Node)`
- the first being the current ID key which is shared between nodes, with a
second Node key uniquely identifying one machine.

This new key can appear inside a nodes `ContactInfo` using the new node key
instead of the identity key as the mapping to CrdsValues. For nodes that are
currently not using more than one machine, this field can be dropped from
the record to not take space up in Gossip.


## Addition of a "No Broadcast" State

The validator pipeline isn't currently equipped to run in a situation where
another node might also currently be live. In particular, even if a node is
running in a mode where it is not voting it will continue to broadcast new
slot shreds when its turn to be leader comes around.

To change this, a new mode (configured by a flag initially,
`--no-broadcasting`) can be added which will prevent a validator node from
acting as leader during its slots even while holding valid keys. This would
allow running multiple nodes sharing key identities to exist at the same time
without trampling on each others slot shreds.


## Addition of Voting/Leading RPC toggles

To make these changes useful to nodes, it should be possible to toggle these
modes on the fly without having to restart the validator. The obvious way to
add these seems to be as RPC endpoints. 



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
   machine removes the possibility of things like fast autoscaling.
2. The changes to implement this are more complex, as a lot of the codebase
   will have to deal with the possibility of receiving data signed by multiple
   different keys. A node key only effects gossip.
3. This doesn't solve the H/A requirement, as it would prevent nodes from
   changing their node setups specifically to deal with the chain losing
   consistency.



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
