# Proof of History

Solana's original whitepaper is still the best resource to learn about the
technique to cryptographically prove that some duration of time passed:

https://solana.com/solana-whitepaper.pdf

## Relationship to consensus mechanisms

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


## Relationship to VDFs

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
