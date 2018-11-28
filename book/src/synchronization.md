# Synchronization

It's possible for a centralized database to process 710,000 transactions per
second on a standard gigabit network if the transactions are, on average, no
more than 176 bytes. A centralized database can also replicate itself and
maintain high availability without significantly compromising that transaction
rate using the distributed system technique known as Optimistic Concurrency
Control [\[H.T.Kung, J.T.Robinson
(1981)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). At
Solana, we're demonstrating that these same theoretical limits apply just as
well to blockchain on an adversarial network. The key ingredient? Finding a way
to share time when nodes can't trust one-another. Once nodes can trust time,
suddenly ~40 years of distributed systems research becomes applicable to
blockchain!

> Perhaps the most striking difference between algorithms obtained by our
> method and ones based upon timeout is that using timeout produces a
> traditional distributed algorithm in which the processes operate
> asynchronously, while our method produces a globally synchronous one in which
> every process does the same thing at (approximately) the same time. Our
> method seems to contradict the whole purpose of distributed processing, which
> is to permit different processes to operate independently and perform
> different functions. However, if a distributed system is really a single
> system, then the processes must be synchronized in some way. Conceptually,
> the easiest way to synchronize processes is to get them all to do the same
> thing at the same time. Therefore, our method is used to implement a kernel
> that performs the necessary synchronization--for example, making sure that
> two different processes do not try to modify a file at the same time.
> Processes might spend only a small fraction of their time executing the
> synchronizing kernel; the rest of the time, they can operate
> independently--e.g., accessing different files. This is an approach we have
> advocated even when fault-tolerance is not required. The method's basic
> simplicity makes it easier to understand the precise properties of a system,
> which is crucial if one is to know just how fault-tolerant the system is.
> [\[L.Lamport
> (1984)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.71.1078)

## Verifiable Delay Functions

A Verifiable Delay Function is conceptually a water clock where its water marks
can be recorded and later verified that the water most certainly passed
through.  Anatoly describes the water clock analogy in detail here:

[water clock analogy](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8)

The same technique has been used in Bitcoin since day one. The Bitcoin feature
is called nLocktime and it can be used to postdate transactions using block
height instead of a timestamp. As a Bitcoin client, you'd use block height
instead of a timestamp if you don't trust the network. Block height turns out
to be an instance of what's being called a Verifiable Delay Function in
cryptography circles. It's a cryptographically secure way to say time has
passed. In Solana, we use a far more granular verifiable delay function, a SHA
256 hash chain, to checkpoint the ledger and coordinate consensus. With it, we
implement Optimistic Concurrency Control and are now well en route towards that
theoretical limit of 710,000 transactions per second.

## Proof of History

[Proof of History overview](https://medium.com/solana-labs/proof-of-history-a-clock-for-blockchain-cf47a61a9274)

### Relationship to Consensus Mechanisms

Most confusingly, a Proof of History (PoH) is more similar to a Verifiable
Delay Function (VDF) than a Proof of Work or Proof of Stake consensus
mechanism. The name unfortunately requires some historical context to
understand. Proof of History was developed by Anatoly Yakovenko in November of
2017, roughly 6 months before we saw a [paper using the term
VDF](https://eprint.iacr.org/2018/601.pdf). At that time, it was commonplace to
publish new proofs of some desirable property used to build most any blockchain
component. Some time shortly after, the crypto community began charting out all
the different consensus mechanisms and because most of them started with "Proof
of", the prefix became synonymous with a "consensus" suffix. Proof of History
is not a consensus mechanism, but it is used to improve the performance of
Solana's Proof of Stake consensus. It is also used to improve the performance
of the replication and storage protocols. To minimize confusion, Solana may
rebrand PoH to some flavor of the term VDF.

### Relationship to VDFs

A desirable property of a VDF is that verification time is very fast. Solana's
approach to verifying its delay function is proportional to the time it took to
create it. Split over a 4000 core GPU, it is sufficiently fast for Solana's
needs, but if you asked the authors the paper cited above, they might tell you
(and have) that Solana's approach is algorithmically slow it shouldn't be
called a VDF. We argue the term VDF should represent the category of verifiable
delay functions and not just the subset with certain performance
characteristics. Until that's resolved, Solana will likely continue using the
term PoH for its application-specific VDF.

Another difference between PoH and VDFs used only for tracking duration, is
that PoH's hash chain includes hashes of any data the application observed.
That data is a double-edged sword. On one side, the data "proves history" -
that the data most certainly existed before hashes after it. On the side, it
means the application can manipulate the hash chain by changing *when* the data
is hashed. The PoH chain therefore does not serve as a good source of
randomness whereas a VDF without that data could. Solana's leader selection
algorithm (TODO: add link), for example, is derived only from the VDF *height*
and not its hash at that height.

