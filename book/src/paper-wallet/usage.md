# Paper Wallet Usage

Solana commands can be run without ever saving a keypair to disk on a machine.
If avoiding writing a private key to disk is a security concern of yours, you've
come to the right place.

{% hint style="warning" %}
Even using this secure input method, it's still possible that a private key gets
written to disk by unencrypted memory swaps. It is the user's responsibility to
protect against this scenario.
{% endhint %}

## Running a Validator

In order to run a validator, you will need to specify an "identity keypair"
which will be used to fund all of the vote transactions signed by your validator.
Rather than specifying a path with `--identity-keypair <PATH>` you can use the
`--ask-seed-phrase` option.

```bash
solana-validator --ask-seed-phrase identity-keypair --ledger ...

[identity-keypair] seed phrase: 🔒
[identity-keypair] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
```

The `--ask-seed-phrase` option accepts multiple keypairs. If you wish to use this
input method for your voting keypair as well you can do the following:

```bash
solana-validator --ask-seed-phrase identity-keypair voting-keypair --ledger ...

[identity-keypair] seed phrase: 🔒
[identity-keypair] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
[voting-keypair] seed phrase: 🔒
[voting-keypair] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
```

Refer to the following page for a comprehensive guide on running a validator:
{% page-ref page="../running-validator/README.md" %}

## Delegating Stake

Solana CLI tooling supports secure keypair input for stake delegation. To do so,
first create a stake account with some SOL. Use the special `ASK` keyword to
trigger a seed phrase input prompt for the stake account and use
`--ask-seed-phrase keypair` to securely input the funding keypair.

```bash
solana create-stake-account ASK 1 SOL --ask-seed-phrase keypair

[stake_account] seed phrase: 🔒
[stake_account] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
[keypair] seed phrase: 🔒
[keypair] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
```

Then, to delegate that stake to a validator, use `--ask-seed-phrase keypair` to
securely input the funding keypair.

```bash
solana delegate-stake --ask-seed-phrase keypair <STAKE_ACCOUNT_PUBKEY> <VOTE_ACCOUNT_PUBKEY>

[keypair] seed phrase: 🔒
[keypair] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue:
```

Refer to the following page for a comprehensive guide on delegating stake:
{% page-ref page="../running-validator/validator-stake.md" %}

---

{% page-ref page="../api-reference/cli.md" %}
