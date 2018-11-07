## Appendix A: Terminology

### Teminology Currently in Use

The following list contains words commonly used throughout the Solana architecture.

* account - a persistent file addressed by pubkey and with tokens tracking its lifetime
* cluster - a set of fullnodes maintaining a single ledger
* finality - the wallclock duration between a leader creating a tick entry and recoginizing
  a supermajority of validator votes with a ledger interpretation that matches the leader's
* fullnode - a full participant in the cluster - either a leader or validator node
* entry - an entry on the ledger - either a tick or a transactions entry
* instruction - the smallest unit of a program that a client can include in a transaction
* keypair - a public and secret key
* mips - millions of instructions per second
* node count - the number of fullnodes participating in a cluster
* program - the code that interprets instructions
* pubkey - the public key of a keypair
* tick - a ledger entry that estimates wallclock duration
* tick height - the Nth tick in the ledger
* tps - transactions per second
* transaction - one or more instructions signed by the client and executed atomically
* transactions entry - a set of transactions that may be executed in parallel


### Terminology Reserved for Future Use

The following keywords do not have any functionality but are reserved by Solana
for potential future use.

* mips - millions of instructions per second
* public key - We currently use `pubkey`
* secret key - Users currently only use `keypair`
