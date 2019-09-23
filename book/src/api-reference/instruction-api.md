# Instruction

For the purposes of building a [Transaction](../transaction.md), a more verbose instruction format is used:

* **Instruction:**
  * **program\_id:** The pubkey of the on-chain program that executes the

    instruction

  * **accounts:** An ordered list of accounts that should be passed to

    the program processing the instruction, including metadata detailing

    if an account is a signer of the transaction and if it is a credit

    only account.

  * **data:** A byte array that is passed to the program executing the

    instruction

A more compact form is actually included in a `Transaction`:

* **CompiledInstruction:**
  * **program\_id\_index:** The index of the `program_id` in the

    `account_keys` list

  * **accounts:** An ordered list of indices into `account_keys`

    specifying the accounds that should be passed to the program

    processing the instruction.

  * **data:** A byte array that is passed to the program executing the

    instruction

