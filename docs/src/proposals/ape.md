== Asynchronous Program Execution ==

Execution of state for fork choice can run asynchronously w.r.t
program execution and computing the BankHash. Prior to this change,
selecting a fork and voting on it required full evaluation of all
the transactions in the bank. With this change only the VoteProgram
is evaluated.

=== Isolation domains ===

To split the VoteProgram apart from the rest of the bank, the runtime
tracks which accounts belong to which isolation domain. A transaction
cannot read accounts from multiple domains in the same transaction,
any transaction that does so aborts.

VoteProgram accounts are isolated in their own domain. Accounts
have a special system instruction that allows them to move between
domains. The move is finalized at the end of the next epoch.

AccountInfo domain fields;

* domain - The isolation domain.

* activation slot - The slot when this account is active in the
domain.

The isolation domain. Currently only two domains exist:

* 0 - The default domain where all things live

* 1 - The Vote Program

==== SystemInstruction::MoveDomains ====

Allows a system account to move domains. Account can only move
once per epoch.  The move is finalized at the end of the next epoch.
This instruction changes the domain and sets the activation slot
to the end of the next epoch.  Currently, only System Accounts are
allowed to move.  Thus creating a new VoteAccount requests a 2 day
warmup.

=== Programs ===

Vote program is the only program that is in domain 1.  System program
is exists in both domain 0 and 1.

=== Execution ===

A transaction can only reference Accounts that are fully active in
its domain. Otherwise the transaction is invalid.  Thus the FeePayer
for votes must be already activated in domain 1.

=== Replay ===

Replay of banks is split up into 2 modes ForkHash and BankHash.

=== ForkHash ===

Computing the ForkHash only evaluates the VoteProgram instructions.
Transactions that include VoteProgram and any non VoteProgram
accounts fail immediately and are ignored.

=== BankHash ===

Computing the BankHash evaluates all the non VoteProgram instructions.
Transactions that include VoteProgram and any non VoteProgram
accounts fail immediately and are ignored.  The LS computation and
finalizing the SystemProgram::MoveDomains are done at the end
of the next epoch.

The BankHash computation must not fall behind the ForkHash by more
than 1 epoch or the cluster will halt.  If a validator's BankHash
falls more than 25% of epoch slots behind ForkHash it should alert
the operator.

=== VoteProgram::Vote ===

The Vote instruction contains ForkHashes only. BankHash computation
is done outside of the root that consensus validators vote on.
Consider mixing in the epoch snapshot hash into PoH once an epoch,
and nodes should continue to gossip snapshot hashes continuously
to ensure they are not experiencing an inconsistency failure.

=== Invalid Fee Payers ===

Transactions that fail due to fee payers unable to pay for the
execution are ignored.  This means that leaders could stuff blocks
with invalid transactions up to the known compute limit. Since the
compute used by each transaction is deterministic, the leader cannot
exceed the total compute available per block, or per writable
account. The leader can only waste ledger space and take up network
bandwidth for transmitting the invalid transaction bytes.

=== PoS Quorum and Leader schedule ===

Vote Accounts with fee payers that are fully activated in the vote
domain and have a minimal balance to cover all the votes for the
duration of the leader schedule are considered for the leader
schedule.
