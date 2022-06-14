== Diet Clients ==

Diet Clients is a protocol for users using the solana network to
get an honest minority security guarantee for confirmations. Users
following the diet client protocol are guaranteed that the confirmation
is valid if at least C/N nodes are honest, where C is some small
configurable minority like 1% of the stake.

Under normal operation a full node operator doesn't depend on the
supermajority of the network for correctness. For example, Circle's
node that processes USDC burn requests will only tell Circle the
request is valid after it locally plays back the ledger. Circle
confirms that the supermajority signed the same result, but doesn't
rely solely on the supermajority. On the other hand, a user that
only talks to an RPC node would at best see the supermajority
signatures for a result.

To get an additional layer of security, diet clients sample the
network such that if a minority of the network believes that the
supermajority is signing invalid block headers, the diet client
stalls confirmations for the user and notifies the user that the
supermajority may be faulty. It is up to the user to acquire a full
node and process the ledger to confirm if the supermajority is
faulty or not.

== Data Availability Sampling ==

For the minority to be able to notify the diet client that the
supermajority is faulty, the minority needs to be able to see the
data the supermajority has signed.

* shreds for block X, must be merklelized into a root - ShredRoot

* ShredRoot must be signed along with the BankHash by the Vote for X

* Nodes must respond to the diet client with a (shred, witness) via an RPC

1. Diet client samples the network for (shred, witness)

2. if minority nodes are available but haven't voted on the block
because of missing shreds, diet client sends them the (shred,
witness)

3. confirmation is considered invalid if supermajority is unable
to provide the data for the block

Diet clients guarantee that minority is able to repair the block
even if supermajority is withholding it.

== Fraud Claim ==

Minority nodes that receive a block and compute a different result
then that was signed by the majority can issue a fraud claim. A
fraud claim burns X sol on any child of the invalid block.

1. Minority receive block data from the network, or via diet client
sampling

2. Minority computes a different BankHash from the header

3. Minority signs a "burn X sol" message and sends it to the diet
client

4. if the total burned exceeds Y, diet client notifies the user
that the majority is faulty and confirmation shouldn't be trusted

5. user should acquire a full node and validate the fraud claim

Fraud claim must be a signature that is handled via user data in
the VoteProgram.  It must be valid for a whole epoch.  The amount
to burn should be enough to prevent spam requests, but doesn't need
to be significant - 100x of the cost to spin up a full node and
verify a snapshot on the cloud.

== Recovery ==

A faulty majority can only be recovered via a socially coordinated
hard fork. Once a diet client receives the fraud claim, users should
socially coordinate over a side channel like discord/twitter and
verify the claim and trigger the hard fork.

Minority signed fraud claims should be redeemable for a portion of
the faulty majority stake.
