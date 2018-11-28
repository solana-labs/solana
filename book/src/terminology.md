## Terminology

### Teminology Currently in Use

The following list contains words commonly used throughout the Solana architecture.

* account - a persistent file addressed by pubkey and with tokens tracking its lifetime
* bootstrap leader - the first fullnode to take the leader role
* control plane - a gossip network connecting all nodes of a cluster
* cluster - a set of fullnodes maintaining a single ledger
* data plane - a multicast network used to efficiently validate entries and gain consensus
* entry - an entry on the ledger - either a tick or a transactions entry
* finality - the wallclock duration between a leader creating a tick entry and recoginizing
  a supermajority of validator votes with a ledger interpretation that matches the leader's
* fullnode - a full participant in the cluster - either a leader or validator node
* genesis block - the first entries of the ledger
* instruction - the smallest unit of a program that a client can include in a transaction
* keypair - a public and secret key
* leader - the role of a fullnode when it is appending entries to the ledger
* node count - the number of fullnodes participating in a cluster
* program - the code that interprets instructions
* pubkey - the public key of a keypair
* replicator - a type of client that stores copies of segments of the ledger
* tick - a ledger entry that estimates wallclock duration
* tick height - the Nth tick in the ledger
* tps - transactions per second
* transaction - one or more instructions signed by the client and executed atomically
* transactions entry - a set of transactions that may be executed in parallel
* validator - the role of a fullnode when it is validating the leader's latest entries


### Terminology Reserved for Future Use

The following keywords do not have any functionality but are reserved by Solana
for potential future use.

* blob - a fraction of a block; the smallest unit sent between validators
* block - the entries generated within a slot
* epoch - the time in which a leader schedule is valid
* light client - a type of client that can verify it's pointing to a valid cluster
* mips - millions of instructions per second
* public key - We currently use `pubkey`
* secret key - Users currently only use `keypair`
* slot - the time in which a single leader may produce entries
* thin client - a type of client that trusts it is communicating with a valid cluster
