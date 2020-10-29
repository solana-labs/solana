---
title: "Overview"
---

An _app_ interacts with a Solana cluster by sending it _transactions_ with one
or more _instructions_. The Solana _runtime_ passes those instructions to
_programs_ deployed by app developers beforehand. An instruction might, for
example, tell a program to transfer _lamports_ from one _account_ to another or
create an interactive contract that governs how lamports are transferred.
Instructions are executed sequentially and atomically for each transaction. If
any instruction is invalid, all account changes in the transaction are
discarded.

To start developing immediately you can build, deploy, and run one of the
[examples](developing/deployed-programs/examples.md).