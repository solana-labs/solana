# Storage-replication Rewards

**Subject to change.**

Archiver-clients download, encrypt and submit PoReps for ledger block sections.3 PoReps submitted to the PoH stream, and subsequently validated, function as evidence that the submitting archiver client is indeed storing the assigned ledger block sections on local hard drive space as a service to the network. Therefore, archiver clients should earn protocol rewards proportional to the amount of storage, and the number of successfully validated PoReps, that they are verifiably providing to the network.

Additionally, archiver clients have the opportunity to capture a portion of slashed bounties \[TBD\] of dishonest validator clients. This can be accomplished by an archiver client submitting a verifiably false PoRep for which a dishonest validator client receives and signs as a valid PoRep. This reward incentive is to prevent lazy validators and minimize validator-archiver collusion attacks, more on this below.

