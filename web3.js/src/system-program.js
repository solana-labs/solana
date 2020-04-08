// @flow

import * as BufferLayout from 'buffer-layout';

import {encodeData, decodeData} from './instruction';
import * as Layout from './layout';
import {NONCE_ACCOUNT_LENGTH} from './nonce-account';
import {PublicKey} from './publickey';
import {SYSVAR_RECENT_BLOCKHASHES_PUBKEY, SYSVAR_RENT_PUBKEY} from './sysvar';
import {Transaction, TransactionInstruction} from './transaction';

/**
 * Create account system transaction params
 * @typedef {Object} CreateAccountParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} newAccountPubkey
 * @property {number} lamports
 * @property {number} space
 * @property {PublicKey} programId
 */
export type CreateAccountParams = {|
  fromPubkey: PublicKey,
  newAccountPubkey: PublicKey,
  lamports: number,
  space: number,
  programId: PublicKey,
|};

/**
 * Transfer system transaction params
 * @typedef {Object} TransferParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} toPubkey
 * @property {number} lamports
 */
export type TransferParams = {|
  fromPubkey: PublicKey,
  toPubkey: PublicKey,
  lamports: number,
|};

/**
 * Assign system transaction params
 * @typedef {Object} AssignParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} programId
 */
export type AssignParams = {|
  fromPubkey: PublicKey,
  programId: PublicKey,
|};

/**
 * Create account with seed system transaction params
 * @typedef {Object} CreateAccountWithSeedParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} newAccountPubkey
 * @property {PublicKey} basePubkey
 * @property {string} seed
 * @property {number} lamports
 * @property {number} space
 * @property {PublicKey} programId
 */
export type CreateAccountWithSeedParams = {|
  fromPubkey: PublicKey,
  newAccountPubkey: PublicKey,
  basePubkey: PublicKey,
  seed: string,
  lamports: number,
  space: number,
  programId: PublicKey,
|};

/**
 * Create nonce account system transaction params
 * @typedef {Object} AssignParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} programId
 */
export type CreateNonceAccountParams = {|
  fromPubkey: PublicKey,
  noncePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  lamports: number,
|};

/**
 * Initialize nonce account system instruction params
 * @typedef {Object} InitializeNonceParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} programId
 */
export type InitializeNonceParams = {|
  noncePubkey: PublicKey,
  authorizedPubkey: PublicKey,
|};

/**
 * Advance nonce account system instruction params
 * @typedef {Object} AdvanceNonceParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} programId
 */
export type AdvanceNonceParams = {|
  noncePubkey: PublicKey,
  authorizedPubkey: PublicKey,
|};

/**
 * Withdraw nonce account system transaction params
 * @typedef {Object} WithdrawNonceParams
 * @property {PublicKey} noncePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} toPubkey
 * @property {number} lamports
 */
export type WithdrawNonceParams = {|
  noncePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  toPubkey: PublicKey,
  lamports: number,
|};

/**
 * Authorize nonce account system transaction params
 * @typedef {Object} AuthorizeNonceParams
 * @property {PublicKey} noncePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} newAuthorizedPubkey
 */
export type AuthorizeNonceParams = {|
  noncePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  newAuthorizedPubkey: PublicKey,
|};

/**
 * System Instruction class
 */
export class SystemInstruction {
  /**
   * Decode a system instruction and retrieve the instruction type.
   */
  static decodeInstructionType(
    instruction: TransactionInstruction,
  ): SystemInstructionType {
    this.checkProgramId(instruction.programId);

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);

    let type;
    for (const t of Object.keys(SYSTEM_INSTRUCTION_LAYOUTS)) {
      if (SYSTEM_INSTRUCTION_LAYOUTS[t].index == typeIndex) {
        type = t;
      }
    }

    if (!type) {
      throw new Error('Instruction type incorrect; not a SystemInstruction');
    }

    return type;
  }

  /**
   * Decode a create account system instruction and retrieve the instruction params.
   */
  static decodeCreateAccount(
    instruction: TransactionInstruction,
  ): CreateAccountParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {lamports, space, programId} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.Create,
      instruction.data,
    );

    return {
      fromPubkey: instruction.keys[0].pubkey,
      newAccountPubkey: instruction.keys[1].pubkey,
      lamports,
      space,
      programId: new PublicKey(programId),
    };
  }

  /**
   * Decode a transfer system instruction and retrieve the instruction params.
   */
  static decodeTransfer(instruction: TransactionInstruction): TransferParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {lamports} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.Transfer,
      instruction.data,
    );

    return {
      fromPubkey: instruction.keys[0].pubkey,
      toPubkey: instruction.keys[1].pubkey,
      lamports,
    };
  }

  /**
   * Decode an assign system instruction and retrieve the instruction params.
   */
  static decodeAssign(instruction: TransactionInstruction): AssignParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 1);

    const {programId} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.Assign,
      instruction.data,
    );

    return {
      fromPubkey: instruction.keys[0].pubkey,
      programId: new PublicKey(programId),
    };
  }

  /**
   * Decode a create account with seed system instruction and retrieve the instruction params.
   */
  static decodeCreateWithSeed(
    instruction: TransactionInstruction,
  ): CreateAccountWithSeedParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {base, seed, lamports, space, programId} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.CreateWithSeed,
      instruction.data,
    );

    return {
      fromPubkey: instruction.keys[0].pubkey,
      newAccountPubkey: instruction.keys[1].pubkey,
      basePubkey: new PublicKey(base),
      seed,
      lamports,
      space,
      programId: new PublicKey(programId),
    };
  }

  /**
   * Decode a nonce initialize system instruction and retrieve the instruction params.
   */
  static decodeNonceInitialize(
    instruction: TransactionInstruction,
  ): InitializeNonceParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);

    const {authorized} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.InitializeNonceAccount,
      instruction.data,
    );

    return {
      noncePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: new PublicKey(authorized),
    };
  }

  /**
   * Decode a nonce advance system instruction and retrieve the instruction params.
   */
  static decodeNonceAdvance(
    instruction: TransactionInstruction,
  ): AdvanceNonceParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);

    decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.AdvanceNonceAccount,
      instruction.data,
    );

    return {
      noncePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
    };
  }

  /**
   * Decode a nonce withdraw system instruction and retrieve the instruction params.
   */
  static decodeNonceWithdraw(
    instruction: TransactionInstruction,
  ): WithdrawNonceParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 5);

    const {lamports} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.WithdrawNonceAccount,
      instruction.data,
    );

    return {
      noncePubkey: instruction.keys[0].pubkey,
      toPubkey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[4].pubkey,
      lamports,
    };
  }

  /**
   * Decode a nonce authorize system instruction and retrieve the instruction params.
   */
  static decodeNonceAuthorize(
    instruction: TransactionInstruction,
  ): AuthorizeNonceParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {authorized} = decodeData(
      SYSTEM_INSTRUCTION_LAYOUTS.AuthorizeNonceAccount,
      instruction.data,
    );

    return {
      noncePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[1].pubkey,
      newAuthorizedPubkey: new PublicKey(authorized),
    };
  }

  /**
   * @private
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(SystemProgram.programId)) {
      throw new Error('invalid instruction; programId is not SystemProgram');
    }
  }

  /**
   * @private
   */
  static checkKeyLength(keys: Array<any>, expectedLength: number) {
    if (keys.length !== expectedLength) {
      throw new Error(
        `invalid instruction; key length mismatch ${keys.length} != ${expectedLength}`,
      );
    }
  }
}

/**
 * An enumeration of valid SystemInstructionType's
 * @typedef {'Create' | 'Assign' | 'Transfer' | 'CreateWithSeed'
 | 'AdvanceNonceAccount' | 'WithdrawNonceAccount' | 'InitializeNonceAccount'
 | 'AuthorizeNonceAccount'} SystemInstructionType
 */
export type SystemInstructionType = $Keys<typeof SYSTEM_INSTRUCTION_LAYOUTS>;

/**
 * An enumeration of valid system InstructionType's
 */
export const SYSTEM_INSTRUCTION_LAYOUTS = Object.freeze({
  Create: {
    index: 0,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
      BufferLayout.ns64('space'),
      Layout.publicKey('programId'),
    ]),
  },
  Assign: {
    index: 1,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('programId'),
    ]),
  },
  Transfer: {
    index: 2,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  CreateWithSeed: {
    index: 3,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('base'),
      Layout.rustString('seed'),
      BufferLayout.ns64('lamports'),
      BufferLayout.ns64('space'),
      Layout.publicKey('programId'),
    ]),
  },
  AdvanceNonceAccount: {
    index: 4,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  WithdrawNonceAccount: {
    index: 5,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  InitializeNonceAccount: {
    index: 6,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('authorized'),
    ]),
  },
  AuthorizeNonceAccount: {
    index: 7,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('authorized'),
    ]),
  },
});

/**
 * Factory class for transactions to interact with the System program
 */
export class SystemProgram {
  /**
   * Public key that identifies the System program
   */
  static get programId(): PublicKey {
    return new PublicKey('11111111111111111111111111111111');
  }

  /**
   * Generate a Transaction that creates a new account
   */
  static createAccount(params: CreateAccountParams): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.Create;
    const data = encodeData(type, {
      lamports: params.lamports,
      space: params.space,
      programId: params.programId.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: params.fromPubkey, isSigner: true, isWritable: true},
        {pubkey: params.newAccountPubkey, isSigner: true, isWritable: true},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that transfers lamports from one account to another
   */
  static transfer(params: TransferParams): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.Transfer;
    const data = encodeData(type, {lamports: params.lamports});

    return new Transaction().add({
      keys: [
        {pubkey: params.fromPubkey, isSigner: true, isWritable: true},
        {pubkey: params.toPubkey, isSigner: false, isWritable: true},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that assigns an account to a program
   */
  static assign(params: AssignParams): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.Assign;
    const data = encodeData(type, {programId: params.programId.toBuffer()});

    return new Transaction().add({
      keys: [{pubkey: params.fromPubkey, isSigner: true, isWritable: true}],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that creates a new account at
   *   an address generated with `from`, a seed, and programId
   */
  static createAccountWithSeed(
    params: CreateAccountWithSeedParams,
  ): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.CreateWithSeed;
    const data = encodeData(type, {
      base: params.basePubkey.toBuffer(),
      seed: params.seed,
      lamports: params.lamports,
      space: params.space,
      programId: params.programId.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: params.fromPubkey, isSigner: true, isWritable: true},
        {pubkey: params.newAccountPubkey, isSigner: false, isWritable: true},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that creates a new Nonce account
   */
  static createNonceAccount(params: CreateNonceAccountParams): Transaction {
    let transaction = SystemProgram.createAccount({
      fromPubkey: params.fromPubkey,
      newAccountPubkey: params.noncePubkey,
      lamports: params.lamports,
      space: NONCE_ACCOUNT_LENGTH,
      programId: this.programId,
    });

    const initParams = {
      noncePubkey: params.noncePubkey,
      authorizedPubkey: params.authorizedPubkey,
    };

    transaction.add(this.nonceInitialize(initParams));
    return transaction;
  }

  /**
   * Generate an instruction to initialize a Nonce account
   */
  static nonceInitialize(
    params: InitializeNonceParams,
  ): TransactionInstruction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.InitializeNonceAccount;
    const data = encodeData(type, {
      authorized: params.authorizedPubkey.toBuffer(),
    });
    const instructionData = {
      keys: [
        {pubkey: params.noncePubkey, isSigner: false, isWritable: true},
        {
          pubkey: SYSVAR_RECENT_BLOCKHASHES_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
      ],
      programId: this.programId,
      data,
    };
    return new TransactionInstruction(instructionData);
  }

  /**
   * Generate an instruction to advance the nonce in a Nonce account
   */
  static nonceAdvance(params: AdvanceNonceParams): TransactionInstruction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.AdvanceNonceAccount;
    const data = encodeData(type);
    const instructionData = {
      keys: [
        {pubkey: params.noncePubkey, isSigner: false, isWritable: true},
        {
          pubkey: SYSVAR_RECENT_BLOCKHASHES_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: params.authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    };
    return new TransactionInstruction(instructionData);
  }

  /**
   * Generate a Transaction that withdraws lamports from a Nonce account
   */
  static nonceWithdraw(params: WithdrawNonceParams): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.WithdrawNonceAccount;
    const data = encodeData(type, {lamports: params.lamports});

    return new Transaction().add({
      keys: [
        {pubkey: params.noncePubkey, isSigner: false, isWritable: true},
        {pubkey: params.toPubkey, isSigner: false, isWritable: true},
        {
          pubkey: SYSVAR_RECENT_BLOCKHASHES_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {
          pubkey: SYSVAR_RENT_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: params.authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that authorizes a new PublicKey as the authority
   * on a Nonce account.
   */
  static nonceAuthorize(params: AuthorizeNonceParams): Transaction {
    const type = SYSTEM_INSTRUCTION_LAYOUTS.AuthorizeNonceAccount;
    const data = encodeData(type, {
      authorized: params.newAuthorizedPubkey.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: params.noncePubkey, isSigner: false, isWritable: true},
        {pubkey: params.authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }
}
