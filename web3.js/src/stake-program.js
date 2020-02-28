// @flow

import * as BufferLayout from 'buffer-layout';

import {encodeData} from './instruction';
import * as Layout from './layout';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_STAKE_HISTORY_PUBKEY,
} from './sysvar';
import {Transaction, TransactionInstruction} from './transaction';
import type {TransactionInstructionCtorFields} from './transaction';

export const STAKE_CONFIG_ID = new PublicKey(
  'StakeConfig11111111111111111111111111111111',
);

export class Authorized {
  staker: PublicKey;
  withdrawer: PublicKey;

  /**
   * Create a new Authorized object
   */
  constructor(staker: PublicKey, withdrawer: PublicKey) {
    this.staker = staker;
    this.withdrawer = withdrawer;
  }
}

export class Lockup {
  unixTimestamp: number;
  epoch: number;
  custodian: PublicKey;

  /**
   * Create a new Lockup object
   */
  constructor(unixTimestamp: number, epoch: number, custodian: PublicKey) {
    this.unixTimestamp = unixTimestamp;
    this.epoch = epoch;
    this.custodian = custodian;
  }
}

/**
 * Stake Instruction class
 */
export class StakeInstruction extends TransactionInstruction {
  _type: StakeInstructionType;

  constructor(
    opts?: TransactionInstructionCtorFields,
    type: StakeInstructionType,
  ) {
    if (
      opts &&
      opts.programId &&
      !opts.programId.equals(StakeProgram.programId)
    ) {
      throw new Error('programId incorrect; not a StakeInstruction');
    }
    super(opts);
    this._type = type;
  }

  static from(instruction: TransactionInstruction): StakeInstruction {
    if (!instruction.programId.equals(StakeProgram.programId)) {
      throw new Error('programId incorrect; not StakeProgram');
    }

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);

    let type;
    for (const t of Object.keys(STAKE_INSTRUCTION_LAYOUTS)) {
      if (STAKE_INSTRUCTION_LAYOUTS[t].index == typeIndex) {
        type = t;
      }
    }
    if (!type) {
      throw new Error('Instruction type incorrect; not a StakeInstruction');
    }
    return new StakeInstruction(
      {
        keys: instruction.keys,
        programId: instruction.programId,
        data: instruction.data,
      },
      type,
    );
  }

  /**
   * Type of StakeInstruction
   */
  get type(): StakeInstructionType {
    return this._type;
  }

  /**
   * The `stake account` public key of the instruction;
   * returns null if StakeInstructionType does not support this field
   */
  get stakePublicKey(): PublicKey | null {
    switch (this.type) {
      case 'Initialize':
      case 'Delegate':
      case 'Authorize':
      case 'Split':
      case 'Withdraw':
      case 'Deactivate':
        return this.keys[0].pubkey;
      default:
        return null;
    }
  }

  /**
   * The `authorized account` public key of the instruction;
   *
   * returns null if StakeInstructionType does not support this field
   */
  get authorizedPublicKey(): PublicKey | null {
    switch (this.type) {
      case 'Delegate':
        return this.keys[5].pubkey;
      case 'Authorize':
        return this.keys[2].pubkey;
      case 'Split':
        return this.keys[2].pubkey;
      case 'Withdraw':
        return this.keys[4].pubkey;
      case 'Deactivate':
        return this.keys[2].pubkey;
      default:
        return null;
    }
  }
}

/**
 * An enumeration of valid StakeInstructionType's
 * @typedef { 'Initialize' | 'Authorize' | 'Delegate' | 'Split' | 'Withdraw'
 | 'Deactivate' } StakeInstructionType
 */
export type StakeInstructionType = $Keys<typeof STAKE_INSTRUCTION_LAYOUTS>;

/**
 * An enumeration of valid stake InstructionType's
 */
export const STAKE_INSTRUCTION_LAYOUTS = Object.freeze({
  Initialize: {
    index: 0,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.authorized(),
      Layout.lockup(),
    ]),
  },
  Authorize: {
    index: 1,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('newAuthorized'),
      BufferLayout.u32('stakeAuthorizationType'),
    ]),
  },
  Delegate: {
    index: 2,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  Split: {
    index: 3,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Withdraw: {
    index: 4,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Deactivate: {
    index: 5,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
});

/**
 * @typedef {Object} StakeAuthorizationType
 * @property (index} The Stake Authorization index (from solana-stake-program)
 */
export type StakeAuthorizationType = {|
  index: number,
|};

/**
 * An enumeration of valid StakeAuthorizationLayout's
 */
export const StakeAuthorizationLayout = Object.freeze({
  Staker: {
    index: 0,
  },
  Withdrawer: {
    index: 1,
  },
});

/**
 * Factory class for transactions to interact with the Stake program
 */
export class StakeProgram {
  /**
   * Public key that identifies the Stake program
   */
  static get programId(): PublicKey {
    return new PublicKey('Stake11111111111111111111111111111111111111');
  }

  /**
   * Max space of a Stake account
   */
  static get space(): number {
    return 4008;
  }

  /**
   * Generate an Initialize instruction to add to a Stake Create transaction
   */
  static initialize(
    stakePubkey: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
  ): TransactionInstruction {
    const type = STAKE_INSTRUCTION_LAYOUTS.Initialize;
    const data = encodeData(type, {
      authorized: {
        staker: authorized.staker.toBuffer(),
        withdrawer: authorized.withdrawer.toBuffer(),
      },
      lockup: {
        unixTimestamp: lockup.unixTimestamp,
        epoch: lockup.epoch,
        custodian: lockup.custodian.toBuffer(),
      },
    });
    const instructionData = {
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
      ],
      programId: this.programId,
      data,
    };
    return new TransactionInstruction(instructionData);
  }

  /**
   * Generate a Transaction that creates a new Stake account at
   *   an address generated with `from`, a seed, and the Stake programId
   */
  static createAccountWithSeed(
    from: PublicKey,
    stakePubkey: PublicKey,
    base: PublicKey,
    seed: string,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  ): Transaction {
    let transaction = SystemProgram.createAccountWithSeed(
      from,
      stakePubkey,
      base,
      seed,
      lamports,
      this.space,
      this.programId,
    );

    return transaction.add(this.initialize(stakePubkey, authorized, lockup));
  }

  /**
   * Generate a Transaction that creates a new Stake account
   */
  static createAccount(
    from: PublicKey,
    stakePubkey: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  ): Transaction {
    let transaction = SystemProgram.createAccount(
      from,
      stakePubkey,
      lamports,
      this.space,
      this.programId,
    );

    return transaction.add(this.initialize(stakePubkey, authorized, lockup));
  }

  /**
   * Generate a Transaction that delegates Stake tokens to a validator
   * Vote PublicKey. This transaction can also be used to redelegate Stake
   * to a new validator Vote PublicKey.
   */
  static delegate(
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    votePubkey: PublicKey,
  ): Transaction {
    const type = STAKE_INSTRUCTION_LAYOUTS.Delegate;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: votePubkey, isSigner: false, isWritable: false},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {
          pubkey: SYSVAR_STAKE_HISTORY_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: STAKE_CONFIG_ID, isSigner: false, isWritable: false},
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that authorizes a new PublicKey as Staker
   * or Withdrawer on the Stake account.
   */
  static authorize(
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    newAuthorized: PublicKey,
    stakeAuthorizationType: StakeAuthorizationType,
  ): Transaction {
    const type = STAKE_INSTRUCTION_LAYOUTS.Authorize;
    const data = encodeData(type, {
      newAuthorized: newAuthorized.toBuffer(),
      stakeAuthorizationType: stakeAuthorizationType.index,
    });

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: true},
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that splits Stake tokens into another stake account
   */
  static split(
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    lamports: number,
    splitStakePubkey: PublicKey,
  ): Transaction {
    let transaction = SystemProgram.createAccount(
      stakePubkey,
      splitStakePubkey,
      0,
      this.space,
      this.programId,
    );
    transaction.instructions[0].keys[0].isSigner = false;
    const type = STAKE_INSTRUCTION_LAYOUTS.Split;
    const data = encodeData(type, {lamports});

    return transaction.add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: splitStakePubkey, isSigner: false, isWritable: true},
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that withdraws deactivated Stake tokens.
   */
  static withdraw(
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    to: PublicKey,
    lamports: number,
  ): Transaction {
    const type = STAKE_INSTRUCTION_LAYOUTS.Withdraw;
    const data = encodeData(type, {lamports});

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: to, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {
          pubkey: SYSVAR_STAKE_HISTORY_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that deactivates Stake tokens.
   */
  static deactivate(
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
  ): Transaction {
    const type = STAKE_INSTRUCTION_LAYOUTS.Deactivate;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }
}
