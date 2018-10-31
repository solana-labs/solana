This document defines the architecture of Solana, a blockchain built from the ground up for scale. The goal of
the architecture is to demonstrate there exists a set of software algorithms that in combination, removes
software as a performance bottleneck, allowing transaction throughput to scale proportionally with network
bandwidth. The architecture goes on to satisfy all three desirable properties of a proper blockchain, that
it not only be scalable, but that it is also secure and decentralized.

With this architecture, we calculate a theoretical upper bound of 710 thousand transactions per second (tps)
on a standard gigabit network and 28.4 million tps on 40 gigabit. In practice, our focus has been on gigabit.
We soak-tested a 150 node permissioned testnet and it is able to maintain a mean transaction throughput of
approximately 200 thousand tps with peaks over 400 thousand.

Furthermore, we have found high throughput extends beyond simple payments, and that this architecture is
also able to perform safe, concurrent execution of programs authored in a general purpose programming
language, such as C. We feel the extension warrants industry focus on an additional performance metric
already common in the CPU industry, millions of *instructions* per second or mips. By measuring
mips, we see that batching instructions within a transaction amortizes the cost of signature
verification, lifting the maximum theoretical instruction throughput up to almost exactly that of
centralized databases.

Lastly, we discuss the relationships between high throughput, security and transaction fees.  Solana's
efficient use hardware drives transaction fees into the ballpark of 1/1000th of a cent. The drop in fees
in turn makes certain denial of service attacks cheaper. We discuss what these attacks look like and
Solana's techniques to defend against them.

