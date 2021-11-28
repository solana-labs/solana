import { TransactionError } from "@solana/web3.js";

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

  if (typeof error === "object" && "InstructionError" in error) {
    const innerError = error["InstructionError"];
    const index = innerError[0] as number;
    const instructionError = innerError[1];

    return {
      index,
      message: getInstructionError(instructionError),
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
