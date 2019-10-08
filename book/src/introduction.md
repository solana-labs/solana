# Introduction

## What is Solana?

Solana is an open source project implementing a new, high-performance, permissionless blockchain. Solana is also the name of a company headquartered in San Francisco that maintains the open source project.

## About this Book

This book describes the Solana open source project, a blockchain built from the ground up for scale. The book covers why Solana is useful, how to use it, how it works, and why it will continue to work long after the company Solana closes its doors. The goal of the Solana architecture is to demonstrate there exists a set of software algorithms that when used in combination to implement a blockchain, removes software as a performance bottleneck, allowing transaction throughput to scale proportionally with network bandwidth. The architecture goes on to satisfy all three desirable properties of a proper blockchain: it is scalable, secure and decentralized.

The architecture describes a theoretical upper bound of 710 thousand transactions per second \(tps\) on a standard gigabit network and 28.4 million tps on 40 gigabit. Furthermore, the architecture supports safe, concurrent execution of programs authored in general purpose programming languages such as C or Rust.

## Disclaimer

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort. It is up to the reader to check and validate their accuracy and truthfulness. Furthermore, nothing in this project constitutes a solicitation for investment.

## History of the Solana Codebase

In November of 2017, Anatoly Yakovenko published a whitepaper describing Proof of History, a technique for keeping time between computers that do not trust one another. From Anatoly's previous experience designing distributed systems at Qualcomm, Mesosphere and Dropbox, he knew that a reliable clock makes network synchronization very simple. When synchronization is simple the resulting network can be blazing fast, bound only by network bandwidth.

Anatoly watched as blockchain systems without clocks, such as Bitcoin and Ethereum, struggled to scale beyond 15 transactions per second worldwide when centralized payment systems such as Visa required peaks of 65,000 tps. Without a clock, it was clear they'd never graduate to being the global payment system or global supercomputer most had dreamed them to be. When Anatoly solved the problem of getting computers that don’t trust each other to agree on time, he knew he had the key to bring 40 years of distributed systems research to the world of blockchain. The resulting cluster wouldn't be just 10 times faster, or a 100 times, or a 1,000 times, but 10,000 times faster, right out of the gate!

Anatoly's implementation began in a private codebase and was implemented in the C programming language. Greg Fitzgerald, who had previously worked with Anatoly at semiconductor giant Qualcomm Incorporated, encouraged him to reimplement the project in the Rust programming language. Greg had worked on the LLVM compiler infrastructure, which underlies both the Clang C/C++ compiler as well as the Rust compiler. Greg claimed that the language's safety guarantees would improve software productivity and that its lack of a garbage collector would allow programs to perform as well as those written in C. Anatoly gave it a shot and just two weeks later, had migrated his entire codebase to Rust. Sold. With plans to weave all the world's transactions together on a single, scalable blockchain, Anatoly called the project Loom.

On February 13th of 2018, Greg began prototyping the first open source implementation of Anatoly's whitepaper. The project was published to GitHub under the name Silk in the loomprotocol organization. On February 28th, Greg made his first release, demonstrating 10 thousand signed transactions could be verified and processed in just over half a second. Shortly after, another former Qualcomm cohort, Stephen Akridge, demonstrated throughput could be massively improved by offloading signature verification to graphics processors. Anatoly recruited Greg, Stephen and three others to co-found a company, then called Loom.

Around the same time, Ethereum-based project Loom Network sprung up and many people were confused about whether they were the same project. The Loom team decided it would rebrand. They chose the name Solana, a nod to a small beach town North of San Diego called Solana Beach, where Anatoly, Greg and Stephen lived and surfed for three years when they worked for Qualcomm. On March 28th, the team created the Solana Labs GitHub organization and renamed Greg's prototype Silk to Solana.

In June of 2018, the team scaled up the technology to run on cloud-based networks and on July 19th, published a 50-node, permissioned, public testnet consistently supporting bursts of 250,000 transactions per second. In a later release in December, called v0.10 Pillbox, the team published a permissioned testnet running 150 nodes on a gigabit network and demonstrated soak tests processing an _average_ of 200 thousand transactions per second with bursts over 500 thousand. The project was also extended to support on-chain programs written in the C programming language and run concurrently in a safe execution environment called BPF.

## What is a Solana Cluster?

A cluster is a set of computers that work together and can be viewed from the outside as a single system. A Solana cluster is a set of independently owned computers working together \(and sometimes against each other\) to verify the output of untrusted, user-submitted programs. A Solana cluster can be utilized any time a user wants to preserve an immutable record of events in time or programmatic interpretations of those events. One use is to track which of the computers did meaningful work to keep the cluster running. Another use might be to track the possession of real-world assets. In each case, the cluster produces a record of events called the ledger. It will be preserved for the lifetime of the cluster. As long as someone somewhere in the world maintains a copy of the ledger, the output of its programs \(which may contain a record of who possesses what\) will forever be reproducible, independent of the organization that launched it.

## What are SOLs?

A SOL is the name of Solana's native token, which can be passed to nodes in a Solana cluster in exchange for running an on-chain program or validating its output. The system may perform micropayments of fractional SOLs and a SOL may be split as many as 34 times. The fractional sol is called a _lamport_. It is named in honor of Solana's biggest technical influence, [Leslie Lamport](https://en.wikipedia.org/wiki/Leslie_Lamport). A lamport has a value of approximately 0.0000000000582 sol \(2^-34\).

