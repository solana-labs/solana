== Asynchronous Program Execution == 

Execution of state for fork choice can run asynchronously w.r.t program execution and computing the BankHash.


=== Isolation domains ===

Accounts include an additional field, the isolation domain. Currently only two domains exist. 
* 1 - the default domain where all things live
* 2 - the Vote Program 
* 3 - moving 2 to 1
* 4 - moving 1 to 2

==== IsolationProgram ====

This program id is used to identify accounts with lamports and data that can move domains.

* IsolationProgram::Withdraw

remove lamports

* IsolationProgram::WriteData

write the data

* IsolationProgram::MoveDomains(1 or 2)

Allows an isolation account to move domains. 

* SystemProgram::CreateVoteAccountFromIsolatedAccount

Allow the system program to consume a IsolationProgram account in domain 2 to become a VoteProgram account.

=== Execution ===

A transaction cannot write to a domain while reading any other domain.  Any transaction that attempt to do so fails. The Vote program is isolated such that any transactions that writes to a VoteProgram account can only read from VoteProgram domain, otherwise the transaction fails.

=== Replay ===

Replay of banks is split up into 2 modes ForkHash and BankHash.

=== ForkHash ===

Computing the ForkHash only evalutes the VoteProgram instructions.  Transactions that include VoteProgram and any non VoteProgram accounts fail immediatly and are ignored.

=== BankHash ===

Computing the BankHash evalutes all the non VoteProgram instructions. Transactions that include VoteProgram and any non VoteProgram accounts fail immediatly and are ignored.  The LS computation and finalizing the IsolationProgram::MoveDomains are done at the epoch boundary for the N+1 epoch.  The BankHash computation must not fall behind the ForkHash by more then 1 epoch or the cluster will halt.

=== VoteProgram::Vote ===

The Vote instruction contains ForkHashes only. BankHash computation is done outside of the root that consensus validators vote on. Consider mixing in the epoch snapshot hash into PoH once an epoch, and nodes should continue to gossip snapshot hashes continuously to ensure they are not experiencing an inconsistency failure.

=== Invalid Fee Payers ===

Transactions that fail due to fee payers unable to pay for the execution are ignored.  This means that leaders could stuff blocks with invalid transactions up to the known compute limit.
