# Create a validator public key

In order to participate in any Tour de SOL dry-runs or stages, you need to register for the Tour de SOL.

See [Registration info](../../registration/README.md).

In order to obtain your allotment of lamports at the start of a Tour de SOL stage or dry run, you need to publish your validator's identity public key under your keybase.io account.

**If these steps are not completed by the cut-off time you will not be able to participate.**

## **Generate Keypair**

1. If you haven't already, generate your validator's identity keypair by running:

   ```bash
     solana-keygen new -o ~/validator-keypair.json
   ```

2. The identity public key can now be viewed by running:

   ```bash
     solana-keygen pubkey ~/validator-keypair.json
   ```

{% hint style="info" %}
Note: The "validator-keypair.json” file is also your \(ed25519\) private key.
{% endhint %}

Your validator identity keypair uniquely identifies your validator within the network. **It is crucial to back-up this information.**

If you don’t back up this information, you WILL NOT BE ABLE TO RECOVER YOUR VALIDATOR, if you lose access to it. If this happens, YOU WILL LOSE YOUR ALLOCATION OF LAMPORTS TOO.

To back-up your validator identify keypair, **back-up your "validator-keypair.json” file to a secure location.**

## Link your Solana pubkey to a Keybase account

You must link your Solana pubkey to a Keybase.io account. The following instructions describe how to do that by installing Keybase on your server.

1. Install [Keybase](https://keybase.io/download) on your machine.
2. Log in to your Keybase account on your server. Create a Keybase account first if you don’t already have one. Here’s a [list of basic Keybase CLI commands](https://keybase.io/docs/command_line/basics).
3. Create a Solana directory in your public file folder: `mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. Publish your validator's identity public key by creating an empty file in your Keybase public file folder in the following format: `/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`. For example:

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. To check your public key was published, ensure you can successfully browse to `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`
