# Traced transaction construction via RPC's transaction simulation

## Problem

With more composition for Solana's program comes, it will be harder and harder
for the higher-level consuming program to correctly construct lower-level
program's instructions by precisely specifying each and every accessed accounts
with the `read_only` or `read_write` flag.

Also, there will be no way to signal the need of additional accounts for the
arbitrarily customized behavior from the lower-level program to the higher-level
one under the standardized instruction API like spl-token, even though we're
about to explore the flexibility of allowing customzed programs conforming to a
standard like the itoken.

These preceding runtime limitations are applicable both to on-chain (instruction
construction for CPI) or off-chain (transaction construction by end-user
interfaces) boundaries likewise.

Lastly, it's practically impossible to change those accessed account addresses
once deployed while retaining compatibility, given the fact the consumers are
expected to hard-code the account addresses for consumption, even if the
deployed programs can be now upgradeable. This severely hampers ongoing
consumed-program's implementation refinement in the areas of data migration and
improved concurrency, by preventing transparent account switching and
sharding with vectorized accounts, respectively.

## Proposed Solution

Extend the `simulateTransaction` RPC method to return more information like
additional account addresses to read and/or write, collected from traced
transaction execution in the simulation.

After that, client signs the final transaction after appending these additional
account addresses and submits it to the cluster via `sendTransaction`.

In other words, programs can dynamically access their _internal_ accounts
implicitly via the newly-added traced syscalls.

## Traced syscalls

The runtime exposes new syscall to provide new account access mechanism which
can be traced:

```rust
fn access_as_unsigned(address: &Pubkey, flags = enum { READ, WRITE}): 
  Result<AccountInfo, Error> { ... }
// NOTE: accessing to signed account in this way will strip the is_signer bit.
// hence that's reflected in the name
```

This syscall behaves differently depending on the BPF execution mode (simulated
or executed):

simulated: load any account at the specified address (if any) and retain any
written data to those accounts in the scope of single transaction simulation.

executed: load any account if the specified address is contained in the list of
tx's account keys.

While MessageProcessor's `verify_accounts` is enforced to these accessed
accounts like normally-accessed accounts, this syscall can bypass the CPI
boundaries by always accessing to the outermost execution environment (the
whole AccountsDB account set while simulated, all of account keys while
executed). This bypass semantics is needed to work with existing CPI.

Then, otherwise noted, the returned AccountInfo should behave exactly same both
in the simulated and executed mode. This is desired for determisnism for
developers and is mandated for security to lightly protect users from bad
programs pretending to be harmless only in simulation.


## example: the associated token account program (demonstrating the problem #1)

```patch
$ git diff
diff --git a/associated-token-account/program/src/processor.rs b/associated-token-account/program/src/processor.rs
index 98eb08b..ff0ad5b 100644
--- a/associated-token-account/program/src/processor.rs
+++ b/associated-token-account/program/src/processor.rs
@@ -22,13 +22,11 @@ pub fn process_instruction(
     let account_info_iter = &mut accounts.iter();
 
     let funder_info = next_account_info(account_info_iter)?;
-    let associated_token_account_info = next_account_info(account_info_iter)?;
     let wallet_account_info = next_account_info(account_info_iter)?;
     let spl_token_mint_info = next_account_info(account_info_iter)?;
-    let system_program_info = next_account_info(account_info_iter)?;
-    let spl_token_program_info = next_account_info(account_info_iter)?;
-    let spl_token_program_id = spl_token_program_info.key;
-    let rent_sysvar_info = next_account_info(account_info_iter)?;
+    let system_program_info = access_as_unsigned(funder_info.owner(), READ)?;
+    let spl_token_program_info = access_as_unsigned(spl_token_mint_info.owner(), READ)?;
+    let rent_sysvar_info = access_as_unsigned(sysvars::rent::id(), READ)?;
 
     let (associated_token_address, bump_seed) = get_associated_token_address_and_bump_seed_internal(
         &wallet_account_info.key,
@@ -36,10 +34,7 @@ pub fn process_instruction(
         program_id,
         &spl_token_program_id,
     );
-    if associated_token_address != *associated_token_account_info.key {
-        msg!("Error: Associated address does not match seed derivation");
-        return Err(ProgramError::InvalidSeeds);
-    }
+    let associated_token_account_info = access_as_unsigned(associated_token_address, READ_WRITE)?;
 
     let associated_token_account_signer_seeds: &[&[_]] = &[
         &wallet_account_info.key.to_bytes(),
```

This reduces the number of require accounts for CPI and clients from 7 to 3, reducing 4.

## example: limited number of NFT transfers per day shared across program:

```rust
pub fn process_transfer(                                              
    program_id: &Pubkey,     
    accounts: &[AccountInfo],                                                                   
    amount: u64,             
    expected_decimals: Option<u8>,    
) -> ProgramResult {
    let counter_address = Pubkey::find_program_address(
        &[  
            &program_id.to_bytes(),
            "counter",
        ],
        program_id,
    );
   let transfer_counter = access_as_unsigned(counter_address, READ_WRITE)?;
   let clock_sysvar = access_as_unsigned(sysvars::clock::id(), READ_WRITE)?;
   let today = yyyymmdd(clock_sysvar);
   if today > transfer_counter.yyyymmdd {
      transfer_counter.yymmdd = today;
      transfer_counter.count = 0;
   } else if transfer_counter > 10 {
      return Err("daily quote of transfer is reached");
   }

   ...
}
```

## example: sharding


```rust
pub fn process_place_order(
  program_id: &Pubkey,     
  market_id: &Pubkey,
  order: &Pubkey,
) {
   let sharded_event_queue = let counter_address = Pubkey::find_program_address(
        &[  
            &market_id.to_bytes(),
            &[ourder_to_bytes()[0] & 0xf0],
        ],
        program_id,
    );
}
```

### example: instruction versioning by traced access after redeploy

```
pub fn check_instruction_version_and_load_data() {
   // this is newly added after the redeploy
   if access_as_unsigned(declare!("OldVersion111111111111111111111111111111111111"), READ).is_ok() {
     return Err("we changed internal data loading implementation. to avoid unexpected malfunction retry");
   }

   // this is changed from OldVersion1111
   access_as_unsigned(declare!("NewVersion111111111111111111111111111111111111"), READ)?

   access_as_unsigned(...);
}
```


## RPC changes

`pre_balances` and `post_balances` needs to be updated?

sample return from simulatedTransaction:

```yml
# these somewhat verbose formatting is for future expansion
transaction:
  instructions
    accesses:
      Bm9UhRBysjeT35ukzriheHKr76yiM1fPyEBMMw2yT9v1: {read: true, write: false}      
      J8H1Gsvnm2CY5oVa1zw1LHHbfvtone8ZGsVBzjkkPbpy: {read: true, write: true}      
    instructions:
      accounts: []
      instruction_type: spl_token_transfer
      instructions: [...]
  accesses: [...]
  pre_balances/pre_token_balances
  post_balances/post_token_balances
  token_approvals
```



also, page-indexed account can be incorporated to find compact combination of existing account indexes to match the required account list


## Other Proposals / alternative implementation


#  or customized entrypoint?

## careful consideration

prevent program  to be simulated or really-executed

## downside

race condition around program deploy (= possible account loading change) => just retry
also, traced program just load dummy addresses for tx versionsing for completeness

# Run these before replaying stage on validator

unavoidable latency.





# Future expansion

Wallet account delta check
Browser-side bpf program simualtion for server-less architecture with online wasm translation
