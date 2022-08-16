export * from './legacy';

/**
 * The message header, identifying signed and read-only account
 */
export type MessageHeader = {
  /**
   * The number of signatures required for this message to be considered valid. The
   * signatures must match the first `numRequiredSignatures` of `accountKeys`.
   */
  numRequiredSignatures: number;
  /** The last `numReadonlySignedAccounts` of the signed keys are read-only accounts */
  numReadonlySignedAccounts: number;
  /** The last `numReadonlySignedAccounts` of the unsigned keys are read-only accounts */
  numReadonlyUnsignedAccounts: number;
};

/**
 * An instruction to execute by a program
 *
 * @property {number} programIdIndex
 * @property {number[]} accounts
 * @property {string} data
 */
export type CompiledInstruction = {
  /** Index into the transaction keys array indicating the program account that executes this instruction */
  programIdIndex: number;
  /** Ordered indices into the transaction keys array indicating which accounts to pass to the program */
  accounts: number[];
  /** The program input data encoded as base 58 */
  data: string;
};
