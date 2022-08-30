# Program Recovery

## Problem

The Upgradable Program Loader makes closing programs an irreversible action.

After closing a program, the [`Program`] points to a non-existent [`ProgramData`] account.
Note that the `Program` account itself still exists, as it is executable, and thus immutable.

  [`Program`]: https://docs.rs/solana-sdk/latest/solana_sdk/bpf_loader_upgradeable/enum.UpgradeableLoaderState.html#variant.Program
  [`ProgramData`]: https://docs.rs/solana-sdk/latest/solana_sdk/bpf_loader_upgradeable/enum.UpgradeableLoaderState.html#variant.ProgramData

Below is an instance of a closed program (as of epoch 343).

```
# Irrecoverable Program account
% solana account optFiKjQpoQ3PvacwnFWaPUAqXCETMJSz2sz8HwPe9B

Public Key: optFiKjQpoQ3PvacwnFWaPUAqXCETMJSz2sz8HwPe9B
Balance: 0.00114144 SOL
Owner: BPFLoaderUpgradeab1e11111111111111111111111
Executable: true
Rent Epoch: 310
Length: 36 (0x24) bytes
0000:   02 00 00 00  89 d1 af 82  0a c8 fc e8  89 1f 84 d6   ................
0010:   ee 7d 70 aa  34 60 99 85  fa 30 3a 08  37 ff b6 4d   .}p.4`...0:.7..M
0020:   b7 20 60 c2                                          . `.

# Irrecoverable ProgramData account
% solana account AGzJVXAdQkyN8mDw619DuAnMyz4ypBJNZHSY7zKKJuvh
Error: AccountNotFound: pubkey=AGzJVXAdQkyN8mDw619DuAnMyz4ypBJNZHSY7zKKJuvh
```

In Solana v1.11, the upgradable loader lacks instructions to recreate a `ProgramData` account that has been closed.
Closed programs are thus permanently irrecoverable.
Any PDAs associated with a closed program would be permanently inaccessible.

This behavior is counterintuitive.
Closed accounts of most other programs can be recreated (e.g. system program, SPL token program)

## Proposed Solution

The upgradable loader should allow the deployer to recreate `ProgramData` accounts for `Program`s with a matching `programdata_address`.

Relax constraints of `UpgradeableLoaderInstruction::DeployWithMaxDataLen`.

```diff
 /// # Account references
 ///   0. `[signer]` The payer account that will pay to create the ProgramData
 ///      account.
 ///   1. `[writable]` The uninitialized ProgramData account.
-///   2. `[writable]` The uninitialized Program account.
+///   2. `[writable]` The Program account.
 ///   3. `[writable]` The Buffer account where the program data has been
 ///      written.  The buffer account's authority must match the program's
 ///      authority
 ///   4. `[]` Rent sysvar.
 ///   5. `[]` Clock sysvar.
 ///   6. `[]` System program (`solana_sdk::system_program::id()`).
 ///   7. `[signer]` The program's authority
 DeployWithMaxDataLen {
     /// Maximum length that the program can be upgraded to.
     max_data_len: usize,
 },

```

Instead of requiring the account 2 to be `Uninitialized`, the instruction should also permit `Program` accounts.

The following rules apply if an initialized program is given to `DeployWithMaxDataLen`:
- The `programdata_address` of the `Program` must match account 1.
- The program account must be a signer.

Deployers can use the `DeployWithData` instruction to recover a closed program, provided they can sign for the program ID.

## Concerns

### Program authority is reset

Closing a program now resets the program authority to the program's address.

Entities in possession of the key of a closed program now gain access to the program and its PDAs.

### Other kinds of irrecoverable programs

This proposal might create a false sense of security and encourage reckless behavior.

Only a specific composition of upgradable loader accounts can be recovered using this method.
There will always be other ways to create a situation where a program is irrecoverable.

### Deliberately disabling programs

Users may have relied on the program close instruction to permanently disable ("brick") a program.
However, the program close method has not been documented as irrecoverable.

To brick a program, users should upgrade the program to an abort instruction and freeze it (by setting the authority to None).
