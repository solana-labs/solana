== Asynchronous Program Execution == 

Execution of state for fork choice can run asynchronously w.r.t program execution and computing the BankHash. Prior to this change, selecting a fork and voting on it required full evaluation of all the transactions in the bank. With this change only the VoteProgram is evaluated.

=== Isolation domains ===

To split the VoteProgram apart from the rest of the bank, the runtime tracks which accounts belong to which isolation domain. A transaction cannot read accounts from multiple domains in the same transaction, any transaction that does so aborts.

VoteProgram accounts are isolated in their own domain. IsolationProgram Accounts have a special instruction that allows them to move between domains. The move is finalized at the end of the next epoch.

IsolationProgramAccount has the following field:
* domain - The isolation domain.

The isolation domain. Currently only two domains exist.

* 1 - the default domain where all things live
* 2 - the Vote Program 

==== IsolationProgram ====

This program id is used to identify accounts with lamports and data that can move domains.

* IsolationProgram::Withdraw

Remove lamports.

* IsolationProgram::MoveDomains(in: 1 or 2)

Allows an isolation account to move domains. Account can only move once per epoch.  The move is finalized at the end of the next epoch.

* SystemProgram::CreateAccountInDomain(in: IsolationProgramAccount, ...)

Allow the system program withdraw lamports from the IsolationProgram account, and create a new account in the isolated domain. 

=== Execution ===

A transaction cannot write to a domain while reading any other domain.  Any transaction that attempt to do so fails. The Vote program is isolated such that any transactions that writes to a VoteProgram account can only read from VoteProgram domain, otherwise the transaction fails.

=== Replay ===

Replay of banks is split up into 2 modes ForkHash and BankHash.

=== ForkHash ===

Computing the ForkHash only evalutes the VoteProgram instructions.  Transactions that include VoteProgram and any non VoteProgram accounts fail immediately and are ignored.

=== BankHash ===

Computing the BankHash evaluates all the non VoteProgram instructions. Transactions that include VoteProgram and any non VoteProgram accounts fail immediately and are ignored.  The LS computation and finalizing the IsolationProgram::MoveDomains are done at the epoch boundary for the N+1 epoch.  The BankHash computation must not fall behind the ForkHash by more then 1 epoch or the cluster will halt.

=== VoteProgram::Vote ===

The Vote instruction contains ForkHashes only. BankHash computation is done outside of the root that consensus validators vote on. Consider mixing in the epoch snapshot hash into PoH once an epoch, and nodes should continue to gossip snapshot hashes continuously to ensure they are not experiencing an inconsistency failure.

=== Invalid Fee Payers ===

Transactions that fail due to fee payers unable to pay for the execution are ignored.  This means that leaders could stuff blocks with invalid transactions up to the known compute limit. Since the compute used by each transaction is deterministic, the leader cannot exceed the total compute available per block, or per writable account. The leader can only waste ledger space and take up network bandwidth for transmitting the invalid transaction bytes.
