import {PublicKey} from '../publickey';

export * from './legacy';
export * from './versioned';
export * from './v0';

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
 * An address table lookup used to load additional accounts
 */
export type MessageAddressTableLookup = {
  accountKey: PublicKey;
  writableIndexes: Array<number>;
  readonlyIndexes: Array<number>;
};

/**
 * An instruction to execute by a program
 *
 * @property {number} programIdIndex
 * @property {number[]} accountKeyIndexes
 * @property {Uint8Array} data
 */
export type MessageCompiledInstruction = {
  /** Index into the transaction keys array indicating the program account that executes this instruction */
  programIdIndex: number;
  /** Ordered indices into the transaction keys array indicating which accounts to pass to the program */
  accountKeyIndexes: number[];
  /** The program input data */
  data: Uint8Array;
};
