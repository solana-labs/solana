// @flow

import * as BufferLayout from 'buffer-layout';

import {Transaction, TransactionInstruction} from './transaction';
import {PublicKey} from './publickey';
import * as Layout from './layout';
import type {TransactionInstructionCtorFields} from './transaction';

/**
 * System Instruction class
 */
export class SystemInstruction extends TransactionInstruction {
  /**
   * Type of SystemInstruction
   */
  type: SystemInstructionType;

  constructor(
    opts?: TransactionInstructionCtorFields,
    type?: SystemInstructionType,
  ) {
    if (
      opts &&
      opts.programId &&
      !opts.programId.equals(SystemProgram.programId)
    ) {
      throw new Error('programId incorrect; not a SystemInstruction');
    }
    super(opts);
    if (type) {
      this.type = type;
    }
  }

  static from(instruction: TransactionInstruction): SystemInstruction {
    if (!instruction.programId.equals(SystemProgram.programId)) {
      throw new Error('programId incorrect; not SystemProgram');
    }

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);
    let type;
    for (const t in SystemInstructionLayout) {
      if (SystemInstructionLayout[t].index == typeIndex) {
        type = SystemInstructionLayout[t];
      }
    }
    if (!type) {
      throw new Error('Instruction type incorrect; not a SystemInstruction');
    }
    return new SystemInstruction(
      {
        keys: instruction.keys,
        programId: instruction.programId,
        data: instruction.data,
      },
      type,
    );
  }

  /**
   * The `from` public key of the instruction;
   * returns null if SystemInstructionType does not support this field
   */
  get fromPublicKey(): PublicKey | null {
    if (
      this.type == SystemInstructionLayout.Create ||
      this.type == SystemInstructionLayout.CreateWithSeed ||
      this.type == SystemInstructionLayout.Transfer
    ) {
      return this.keys[0].pubkey;
    }
    return null;
  }

  /**
   * The `to` public key of the instruction;
   * returns null if SystemInstructionType does not support this field
   */
  get toPublicKey(): PublicKey | null {
    if (
      this.type == SystemInstructionLayout.Create ||
      this.type == SystemInstructionLayout.CreateWithSeed ||
      this.type == SystemInstructionLayout.Transfer
    ) {
      return this.keys[1].pubkey;
    }
    return null;
  }

  /**
   * The `amount` or `lamports` of the instruction;
   * returns null if SystemInstructionType does not support this field
   */
  get amount(): number | null {
    const data = this.type.layout.decode(this.data);
    if (this.type == SystemInstructionLayout.Transfer) {
      return data.amount;
    } else if (
      this.type == SystemInstructionLayout.Create ||
      this.type == SystemInstructionLayout.CreateWithSeed
    ) {
      return data.lamports;
    }
    return null;
  }
}

/**
 * @typedef {Object} SystemInstructionType
 * @property (index} The System Instruction index (from solana-sdk)
 * @property (BufferLayout} The BufferLayout to use to build data
 */
type SystemInstructionType = {|
  index: number,
  layout: typeof BufferLayout,
|};

/**
 * An enumeration of valid SystemInstructionTypes
 */
const SystemInstructionLayout = Object.freeze({
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
      BufferLayout.ns64('amount'),
    ]),
  },
  CreateWithSeed: {
    index: 3,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.rustString('seed'),
      BufferLayout.ns64('lamports'),
      BufferLayout.ns64('space'),
      Layout.publicKey('programId'),
    ]),
  },
});

/**
 * Populate a buffer of instruction data using the SystemInstructionType
 */
function encodeData(type: SystemInstructionType, fields: Object): Buffer {
  const allocLength =
    type.layout.span >= 0 ? type.layout.span : Layout.getAlloc(type, fields);
  const data = Buffer.alloc(allocLength);
  const layoutFields = Object.assign({instruction: type.index}, fields);
  type.layout.encode(layoutFields, data);
  return data;
}

/**
 * Factory class for transactions to interact with the System program
 */
export class SystemProgram {
  /**
   * Public key that identifies the System program
   */
  static get programId(): PublicKey {
    return new PublicKey(
      '0x000000000000000000000000000000000000000000000000000000000000000',
    );
  }

  /**
   * Generate a Transaction that creates a new account
   */
  static createAccount(
    from: PublicKey,
    newAccount: PublicKey,
    lamports: number,
    space: number,
    programId: PublicKey,
  ): Transaction {
    const type = SystemInstructionLayout.Create;
    const data = encodeData(type, {
      lamports,
      space,
      programId: programId.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true, isWritable: true},
        {pubkey: newAccount, isSigner: true, isWritable: true},
      ],
      programId: SystemProgram.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that transfers lamports from one account to another
   */
  static transfer(from: PublicKey, to: PublicKey, amount: number): Transaction {
    const type = SystemInstructionLayout.Transfer;
    const data = encodeData(type, {amount});

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true, isWritable: true},
        {pubkey: to, isSigner: false, isWritable: true},
      ],
      programId: SystemProgram.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that assigns an account to a program
   */
  static assign(from: PublicKey, programId: PublicKey): Transaction {
    const type = SystemInstructionLayout.Assign;
    const data = encodeData(type, {programId: programId.toBuffer()});

    return new Transaction().add({
      keys: [{pubkey: from, isSigner: true, isWritable: true}],
      programId: SystemProgram.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that creates a new account at
   *   an address generated with `from`, a seed, and programId
   */
  static createAccountWithSeed(
    from: PublicKey,
    newAccount: PublicKey,
    seed: string,
    lamports: number,
    space: number,
    programId: PublicKey,
  ): Transaction {
    const type = SystemInstructionLayout.CreateWithSeed;
    const data = encodeData(type, {
      seed,
      lamports,
      space,
      programId: programId.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true, isWritable: true},
        {pubkey: newAccount, isSigner: false, isWritable: true},
      ],
      programId: SystemProgram.programId,
      data,
    });
  }
}
