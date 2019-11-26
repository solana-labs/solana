# Paper Wallet Usage

Solana commands can be run without ever saving a keypair to disk on a machine. If avoiding writing a private key to disk is a security concern of yours, you've come to the right place.

{% hint style="warning" %}
Even using this secure input method, it's still possible that a private key gets written to disk by unencrypted memory swaps. It is the user's responsibility to protect against this scenario.
{% endhint %}

## Running a Validator

{% hint style="into" %}
This page is not meant to be a full guide for running a validator. Please visit [Running a Validator](running-validator/README.md) for a comprehensive guide
{% endhint %}

```bash
solana-validator --ask-seed-phrase identity-keypair voting-keypair ..
```

---

{% page-ref page="api-reference/cli.md" %}
