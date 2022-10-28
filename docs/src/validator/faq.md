---
title: Validator Frequently Asked Questions
sidebar_label: Frequently Asked Questions
---

### How long does it take for new stake to show up for my validator?

You will have to wait until the next epoch. Each epoch is approximately 2 days. You can get information about the current epoch using `solana epoch-info`.

### Can I run my validator at home?

Anyone can join the cluster including home users. However, you must make sure that your system can perform well and keep up with the cluster. Many home internet connections are not suitable to run a Solana validator.

See the [validator system requirements](../running-validator/validator-reqs.md) for more information.

### Is running a validator the same as mining?

No, a validator uses proof of stake instead of proof of work. See [what is a validator?](./overview/what-is-a-validator.md#proof-of-stake).

### What are the 3 keys (identity, voting, withdrawer) used for in my validator?

- _Identity_: this keypair is used as a payment account for the sol transaction fees incurred on the network. It must be stored on the validator server for the `solana-validator` program to operate.

- _Vote_: this keypair is used to create your [vote account](../terminology.md#ledger-vote). The account is identified by the keypair's pubkey. Once the vote account is created, this keypair can be deleted.

- _Withdrawer_: this keypair allows the operator of the validator to withdraw funds awarded to the validator. It should **NOT** be kept on the validator itself. This keypair should be stored in a secure place like a hardware wallet or key storage system.

### Should I store my withdrawer key on my validator?

No, do not store your withdrawer key on your validator.

### Can my withdrawer key be the same as my identity key?

No, the withdrawer key should not be stored on the validator and should not be the same keypair.

### What does catching up to the cluster mean?

When a validator node starts up, it must either download a snapshot or use a local snapshot. This snapshot represents the state of the cluster, but the cluster may have advanced beyond your local snapshot by the time your node starts. In order for your node to be involved in voting, it must download all the blocks that it does not have in its snapshot. This process is known as catching up to the cluster. You will have to do this almost any time you stop and then restart your validator service.

### How can you check if your node has caught up to the cluster?

`solana catchup <pubkey>`

### How do you find your pubkey?

`solana-keygen pubkey validator-keypair.json`
