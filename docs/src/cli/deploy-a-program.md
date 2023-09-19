---
title: Deploy a Program
---

Developers can deploy on-chain [programs](terminology.md#program) (often called
smart contracts elsewhere) with the Solana tools.

To learn about developing and executing programs on Solana, start with the
[intro to Solana programs](developing/intro/programs.md) and then dig into the
details of [on-chain programs](developing/on-chain-programs/overview.md).

To deploy a program, use the Solana tools to interact with the on-chain loader
to:

- Initialize a program account
- Upload the program's shared object to the program account's data buffer
- Verify the uploaded program
- Finalize the program by marking the program account executable.

Once deployed, anyone can execute the program by sending transactions that
reference it to the cluster.

## Usage

### Deploy a program

To deploy a program, you will need the location of the program's shared object
(the program binary .so)

```bash
solana program deploy <PROGRAM_FILEPATH>
```

Successful deployment will return the program id of the deployed program, for
example:

```bash
Program Id: 3KS2k14CmtnuVv2fvYcvdrNgC94Y11WETBpMUGgXyWZL
```

Specify the keypair in the deploy command to deploy to a specific program id:

```bash
solana program deploy --program-id <KEYPAIR_FILEPATH> <PROGRAM_FILEPATH>
```

If the program id is not specified on the command line the tools will first look
for a keypair file matching the `<PROGRAM_FILEPATH>`, or internally generate a
new keypair.

A matching program keypair file is in the same directory as the program's shared
object, and named <PROGRAM_NAME>-keypair.json. Matching program keypairs are
generated automatically by the program build tools:

```bash
./path-to-program/program.so
./path-to-program/program-keypair.json
```

### Showing a program account

To get information about a deployed program:

```bash
solana program show <ACCOUNT_ADDRESS>
```

An example output looks like:

```bash
Program Id: 3KS2k14CmtnuVv2fvYcvdrNgC94Y11WETBpMUGgXyWZL
Owner: BPFLoaderUpgradeab1e11111111111111111111111
ProgramData Address: EHsACWBhgmw8iq5dmUZzTA1esRqcTognhKNHUkPi4q4g
Authority: FwoGJNUaJN2zfVEex9BB11Dqb3NJKy3e9oY3KTh9XzCU
Last Deployed In Slot: 63890568
Data Length: 5216 (0x1460) bytes
```

- `Program Id` is the address that can be referenced in an instruction's
  `program_id` field when invoking a program.
- `Owner`: The loader this program was deployed with.
- `ProgramData Address` is the account associated with the program account that
  holds the program's data (shared object).
- `Authority` is the program's upgrade authority.
- `Last Deployed In Slot` is the slot in which the program was last deployed.
- `Data Length` is the size of the space reserved for deployments. The actual
  space used by the currently deployed program may be less.

### Redeploy a program

A program can be redeployed to the same address to facilitate rapid development,
bug fixes, or upgrades. Matching keypair files are generated once so that
redeployments will be to the same program address.

The command looks the same as the deployment command:

```bash
solana program deploy <PROGRAM_FILEPATH>
```

By default, programs are deployed to accounts that are twice the size of the
original deployment. Doing so leaves room for program growth in future
redeployments. But, if the initially deployed program is very small  and then
later grows substantially, the redeployment may fail. To avoid this, specify
a `max_len` that is at least the size (in bytes) that the program is expected
to become (plus some wiggle room).

```bash
solana program deploy --max-len 200000 <PROGRAM_FILEPATH>
```

Note that program accounts are required to be
[rent-exempt](developing/programming-model/accounts.md#rent-exemption), and the
`max-len` is fixed after initial deployment, so any SOL in the program accounts
is locked up permanently.

### Resuming a failed deploy

If program deployment fails, there will be a hanging intermediate buffer account
that contains a non-zero balance. In order to recoup that balance you may
resume a failed deployment by providing the same intermediate buffer to a new
call to `deploy`.

Deployment failures will print an error message specifying the seed phrase
needed to recover the generated intermediate buffer's keypair:

```
==================================================================================
Recover the intermediate account's ephemeral keypair file with
`solana-keygen recover` and the following 12-word seed phrase:
==================================================================================
valley flat great hockey share token excess clever benefit traffic avocado athlete
==================================================================================
To resume a deploy, pass the recovered keypair as
the [BUFFER_SIGNER] to `solana program deploy` or `solana program write-buffer'.
Or to recover the account's lamports, pass it as the
[BUFFER_ACCOUNT_ADDRESS] argument to `solana program drain`.
==================================================================================
```

To recover the keypair:

```bash
solana-keygen recover -o <KEYPAIR_PATH>
```

When asked, enter the 12-word seed phrase.

Then issue a new `deploy` command and specify the buffer:

```bash
solana program deploy --buffer <KEYPAIR_PATH> <PROGRAM_FILEPATH>
```

### Closing program and buffer accounts, and reclaiming their lamports

Both program and buffer accounts can be closed and their lamport balances
transferred to a recipient's account.

If deployment fails there will be a left over buffer account that holds
lamports. The buffer account can either be used to [resume a
deploy](#resuming-a-failed-deploy) or closed.

The program or buffer account's authority must be present to close an account,
to list all the open program or buffer accounts that match the default
authority:

```bash
solana program show --programs
solana program show --buffers
```

To specify a different authority:

```bash
solana program show --programs --buffer-authority <AURTHORITY_ADRESS>
solana program show --buffers --buffer-authority <AURTHORITY_ADRESS>
```

To close a single account:

```bash
solana program close <BADDRESS>
```

To close a single account and specify a different authority than the default:

```bash
solana program close <ADDRESS> --buffer-authority <KEYPAIR_FILEPATH>
```

To close a single account and specify a different recipient than the default:

```bash
solana program close <ADDRESS> --recipient <RECIPIENT_ADDRESS>
```

To close all the buffer accounts associated with the current authority:

```bash
solana program close --buffers
```

To show all buffer accounts regardless of the authority

```bash
solana program show --buffers --all
```

### Set a program's upgrade authority

The program's upgrade authority must to be present to deploy a program. If no
authority is specified during program deployment, the default keypair is used as
the authority. This is why redeploying a program in the steps above didn't
require an authority to be explicitly specified.

The authority can be specified during deployment:

```bash
solana program deploy --upgrade-authority <UPGRADE_AUTHORITY_SIGNER> <PROGRAM_FILEPATH>
```

Or after deployment and using the default keypair as the current authority:

```bash
solana program set-upgrade-authority <PROGRAM_ADDRESS> --new-upgrade-authority <NEW_UPGRADE_AUTHORITY>
```

Or after deployment and specifying the current authority:

```bash
solana program set-upgrade-authority <PROGRAM_ADDRESS> --upgrade-authority <UPGRADE_AUTHORITY_SIGNER> --new-upgrade-authority <NEW_UPGRADE_AUTHORITY>
```

### Immutable programs

A program can be marked immutable, which prevents all further redeployments, by
specifying the `--final` flag during deployment:

```bash
solana program deploy <PROGRAM_FILEPATH> --final
```

Or anytime after:

```bash
solana program set-upgrade-authority <PROGRAM_ADDRESS> --final
```

### Dumping a program to a file

The deployed program may be dumped back to a local file:

```bash
solana program dump <ACCOUNT_ADDRESS> <OUTPUT_FILEPATH>
```

The dumped file will be in the same as what was deployed, so in the case of a
shared object, the dumped file will be a fully functional shared object. Note
that the `dump` command dumps the entire data space, which means the output file
will have trailing zeros after the shared object's data up to `max_len`.
Sometimes it is useful to dump and compare a program to ensure it matches a
known program binary. The original program file can be zero-extended, hashed,
and compared to the hash of the dumped file.

```bash
$ solana dump <ACCOUNT_ADDRESS> dump.so
$ cp original.so extended.so
$ truncate -r dump.so extended.so
$ sha256sum extended.so dump.so
```

### Using an intermediary Buffer account

Instead of deploying directly to the program account, the program can be written
to an intermediary buffer account. Intermediary accounts can be useful for things
like multi-entity governed programs where the governing members fist verify the
intermediary buffer contents and then vote to allow an upgrade using it.

```bash
solana program write-buffer <PROGRAM_FILEPATH>
```

Buffer accounts support authorities like program accounts:

```bash
solana program set-buffer-authority <BUFFER_ADDRESS> --new-buffer-authority <NEW_BUFFER_AUTHORITY>
```

One exception is that buffer accounts cannot be marked immutable like program
accounts can, so they don't support `--final`.

The buffer account, once entirely written, can be passed to `deploy` to deploy
the program:

```bash
solana program deploy --program-id <PROGRAM_ADDRESS> --buffer <BUFFER_ADDRESS>
```

Note, the buffer's authority must match the program's upgrade authority.

Buffers also support `show` and `dump` just like programs do.
