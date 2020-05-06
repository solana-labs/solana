# Validator Groups

The current design of gossip identifies a single validator node with an
identity key. This means a validator that wishes to run more than one node
would have to share that key. The current design of gossip however will
associate this key with a set of network addresses, so each node will begin to
trample each others gossip data.

This proposal defines the idea of Validator Groups - these groups will share a
single vote account, but only one node is ever the "active" voter when it is
their turn to vote. Managing which node is meant to vote is up to the validator
to manage.



# Overview of Required Changes

## Addition of a Node Key

For a validator to share state among machines, it needs some way of being
grouped. One way to do this would be to modify the `VoteState` to allow for
multiple validator identity keys to sign. This however has a large flaw.
In order for a set of validator nodes to associate with the same vote account
there are two possible solutions:

1) Allow multiple validator identity keys to be associated with a vote account.
2) Allow multiple machines to share the same identity key.

The main problem with the first approach is it requires on-chain consensus to
manage this set. The key purpose of allowing a validator group though is to
allow a validator to quickly change their topology at short notice. This allows
things like H/A or fast failover.

The latter approach is the one this proposal aims to implement, by allowing
multiple machines to share a validator identity the topology of the hardware
itself is removed from the chain. Secure use of this key is then the
responsibility of the validator themselves.

In order to achieve this, a validator identity key in gossip is treated as a
grouping key. Ephemeral "machine" keys can then be used to distinguish each
machine utilizing this key.


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
