import { TransactionError } from "@solana/web3.js";

const TrasactionErrorMessage: Map<string, string> = new Map([
  [
    "AccountInUse",
    "An account is already being processed in another transaction in a way that does not support parallelism",
  ],
  [
    "AccountLoadedTwice",
    "A Pubkey appears twice in the transaction’s account_keys. Instructions can reference Pubkeys more than once but the message must contain a list with no duplicate keys",
  ],
  [
    "AccountNotFound",
    "Attempt to debit an account but found no record of a prior credit.",
  ],

  ["ProgramAccountNotFound", "Attempt to load a program that does not exist"],

  [
    "InsufficientFundsForFee",
    "The from Pubkey does not have sufficient balance to pay the fee to schedule the transaction",
  ],

  [
    "InvalidAccountForFee",
    "This account may not be used to pay transaction fees",
  ],

  [
    "AlreadyProcessed",
    "The bank has seen this transaction before. This can occur under normal operation when a UDP packet is duplicated, as a user error from a client not updating its recent_blockhash, or as a double-spend attack",
  ],
  [
    "BlockhashNotFound",
    "The bank has not seen the given recent_blockhash or the transaction is too old and the recent_blockhash has been discarded.",
  ],

  ["InstructionError", "Instruction at : 1"],

  ["CallChainTooDeep", "Loader call chain is too deep"],

  [
    "MissingSignatureForFee",
    "Transaction requires a fee but has no signature present",
  ],

  ["InvalidAccountIndex", "Transaction contains an invalid account reference"],

  ["SignatureFailure", "Transaction did not pass signature verification"],

  [
    "InvalidProgramForExecution",
    "This program may not be used for executing instructions",
  ],

  [
    "SanitizeFailure",
    "Transaction failed to sanitize accounts offsets correctly implies that account locks are not taken for this TX, and should not be unlocked.",
  ],

  [
    "AccountBorrowOutstanding",
    "Transaction processing left an account with an outstanding borrowed reference",
  ],

  [
    "WouldExceedMaxBlockCostLimit",
    "Transaction would exceed max Block Cost Limit",
  ],

  ["UnsupportedVersion", "Transaction version is unsupported"],

  [
    "InvalidWritableAccount",
    "Transaction loads a writable account that cannot be written",
  ],

  [
    "WouldExceedMaxAccountCostLimit",
    "Transaction would exceed max account limit within the block",
  ],

  [
    "WouldExceedAccountDataBlockLimit",
    "Transaction would exceed account data limit within the block",
  ],

  ["TooManyAccountLocks", "Transaction locked too many accounts"],

  ["AddressLookupTableNotFound", "Address lookup table not found"],

  [
    "InvalidAddressLookupTableOwner",
    "Attempted to lookup addresses from an account owned by the wrong program",
  ],

  [
    "InvalidAddressLookupTableData",
    "Attempted to lookup addresses from an invalid account",
  ],

  [
    "InvalidAddressLookupTableIndex",
    "Address table lookup uses an invalid index",
  ],

  [
    "InvalidRentPayingAccount",
    "Transaction leaves an account with a lower balance than rent-exempt minimum",
  ],

  [
    "WouldExceedMaxVoteCostLimit",
    "Transaction would exceed max Vote Cost Limit",
  ],
  ["DuplicateInstruction", "Constains duplicate instructions: {0}"],

  [
    "WouldExceedAccountDataTotalLimit",
    "Transaction would exceed total account data limit",
  ],

  [
    "InsufficientFundsForRent",
    "Transaction results in an account without insufficient funds for rent",
  ],
]);

const instructionErrorMessage: Map<string, string> = new Map([
  ["GenericError", "generic instruction error"],
  ["InvalidArgument", "invalid program argument"],
  ["InvalidInstructionData", "invalid instruction data"],
  ["InvalidAccountData", "invalid account data for instruction"],
  ["AccountDataTooSmall", "account data too small for instruction"],
  ["InsufficientFunds", "insufficient funds for instruction"],
  ["IncorrectProgramId", "incorrect program id for instruction"],
  ["MissingRequiredSignature", "missing required signature for instruction"],
  [
    "AccountAlreadyInitialized",
    "instruction requires an uninitialized account",
  ],
  ["UninitializedAccount", "instruction requires an initialized account"],
  [
    "UnbalancedInstruction",
    "sum of account balances before and after instruction do not match",
  ],
  ["ModifiedProgramId", "instruction modified the program id of an account"],
  [
    "ExternalAccountLamportSpend",
    "instruction spent from the balance of an account it does not own",
  ],
  [
    "ExternalAccountDataModified",
    "instruction modified data of an account it does not own",
  ],
  [
    "ReadonlyLamportChange",
    "instruction changed the balance of a read-only account",
  ],
  ["ReadonlyDataModified", "instruction modified data of a read-only account"],
  ["DuplicateAccountIndex", "instruction contains duplicate accounts"],
  ["ExecutableModified", "instruction changed executable bit of an account"],
  ["RentEpochModified", "instruction modified rent epoch of an account"],
  ["NotEnoughAccountKeys", "insufficient account keys for instruction"],
  ["AccountDataSizeChanged", "non-system instruction changed account size"],
  ["AccountNotExecutable", "instruction expected an executable account"],
  [
    "AccountBorrowFailed",
    "instruction tries to borrow reference for an account which is already borrowed",
  ],
  [
    "AccountBorrowOutstanding",
    "instruction left account with an outstanding borrowed reference",
  ],
  [
    "DuplicateAccountOutOfSync",
    "instruction modifications of multiply-passed account differ",
  ],
  ["Custom", "custom program error: {0}"],
  ["InvalidError", "program returned invalid error code"],
  ["ExecutableDataModified", "instruction changed executable accounts data"],
  [
    "ExecutableLamportChange",
    "instruction changed the balance of a executable account",
  ],
  ["ExecutableAccountNotRentExempt", "executable accounts must be rent exempt"],
  ["UnsupportedProgramId", "Unsupported program id"],
  ["CallDepth", "Cross-program invocation call depth too deep"],
  ["MissingAccount", "An account required by the instruction is missing"],
  [
    "ReentrancyNotAllowed",
    "Cross-program invocation reentrancy not allowed for this instruction",
  ],
  [
    "MaxSeedLengthExceeded",
    "Length of the seed is too long for address generation",
  ],
  ["InvalidSeeds", "Provided seeds do not result in a valid address"],
  ["InvalidRealloc", "Failed to reallocate account data"],
  ["ComputationalBudgetExceeded", "Computational budget exceeded"],
  [
    "PrivilegeEscalation",
    "Cross-program invocation with unauthorized signer or writable account",
  ],
  [
    "ProgramEnvironmentSetupFailure",
    "Failed to create program execution environment",
  ],
  ["ProgramFailedToComplete", "Program failed to complete"],
  ["ProgramFailedToCompile", "Program failed to compile"],
  ["Immutable", "Account is immutable"],
  ["IncorrectAuthority", "Incorrect authority provided"],
  ["BorshIoError", "Failed to serialize or deserialize account data: {0}"],
  [
    "AccountNotRentExempt",
    "An account does not have enough lamports to be rent-exempt",
  ],
  ["InvalidAccountOwner", "Invalid account owner"],
  ["ArithmeticOverflow", "Program arithmetic overflowed"],
  ["UnsupportedSysvar", "Unsupported sysvar"],
  ["IllegalOwner", "Provided owner is not allowed"],
]);

export type ProgramError = {
  index: number;
  message: string;
};

export function getTransactionInstructionError(
  error?: TransactionError | null
): ProgramError | undefined {
  if (!error) {
    return;
  }

  if (typeof error === "object") {
    let instructionErrorKey = "InstructionError";
    let transactionErrorKey = "TransactionError";

    let index: number = 0;
    let errors: string = "";
    if (instructionErrorKey in error) {
      //@ts-ignore
      const innerError = error[instructionErrorKey];
      index += innerError[0] as number;
      const instructionError = innerError[1];
      errors += getInstructionError(instructionError);
    }

    if (transactionErrorKey in error) {
      //@ts-ignore
      const innerError = error[transactionErrorKey];
      index += (innerError[0] as number) + 1;
      const instructionError = innerError[1];
      errors += getTransactionError(instructionError);
    }

    return {
      index,
      message: errors,
    };
  }
}

function getInstructionError(error: any): string {
  let out;
  let value;

  if (typeof error === "string") {
    const message = instructionErrorMessage.get(error);
    if (message) {
      return message;
    }
  } else if ("Custom" in error) {
    out = instructionErrorMessage.get("Custom");
    value = error["Custom"];
  } else if ("BorshIoError" in error) {
    out = instructionErrorMessage.get("BorshIoError");
    value = error["BorshIoError"];
  }

  if (out && value) {
    return out.replace("{0}", value);
  }

  return "Unknown instruction error";
}

function getTransactionError(error: any): string {
  let out;
  let value;

  if (typeof error === "string") {
    const message = TrasactionErrorMessage.get(error);
    if (message) {
      return message;
    }
  } else if ("InstructionError" in error) {
    out = instructionErrorMessage.get("InstructionError");
    value = error["InstructionError"];
  } else if ("DuplicateInstruction" in error) {
    out = instructionErrorMessage.get("DuplicateInstruction");
    value = error["DuplicateInstruction"];
  }

  if (out && value) {
    return out.replace("{0}", value);
  }

  return "Unknown instruction error";
}
