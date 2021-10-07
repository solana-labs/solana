---
title: Cost Model
---

## Motivation

Needs a method to calculate transaction's cost that captures the work required by validator to process it.

## Solution

### Transaction Cost calculation
Cost is measured in `compute unit`, which describes required computational work (eg., CPU).

Transaction cost is closely echoing proposed Comprehensive Fee Structure (#16984), it is the sum of following components:
1. signature cost, is the number of signatures in a transaction multiplied with fixed signature cost;
2. write lock cost, is number of writable accounts in transaction multiplied with fixed write_lock cost;
3. data byte cost, is transaction's data size multiple by fixed cost per bytes rate;
4. instruction execution cost, is the sum of cost of instructions in transaction. System built in instruction has fixed cost, while bpf instructions are collected in runtime.

Methods to determine fixed cost for above components are described in issue #19627.
å
### Transaction Cost composition
*** subject to change ***
In addition to above components, Transaction Cost also tracks the writable accounts in transactions. This additional information could be used by block producers to limit the number of transactions for the same account, by limiting cost allowed per account, hence improving block's parallelism.

##


~
~
~
~
~
~
