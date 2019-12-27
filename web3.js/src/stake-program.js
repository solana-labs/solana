// @flow

import * as BufferLayout from 'buffer-layout';
import hasha from 'hasha';

import {encodeData} from './instruction';
import type {InstructionType} from './instruction';
import * as Layout from './layout';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_REWARDS_PUBKEY,
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
  /**
   * Type of StakeInstruction
   */
  type: InstructionType;

  constructor(opts?: TransactionInstructionCtorFields, type?: InstructionType) {
    if (
      opts &&
      opts.programId &&
      !opts.programId.equals(StakeProgram.programId)
    ) {
      throw new Error('programId incorrect; not a StakeInstruction');
    }
    super(opts);
    if (type) {
      this.type = type;
    }
  }

  static from(instruction: TransactionInstruction): StakeInstruction {
    if (!instruction.programId.equals(StakeProgram.programId)) {
      throw new Error('programId incorrect; not StakeProgram');
    }

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);
    let type;
    for (const t in StakeInstructionLayout) {
      if (StakeInstructionLayout[t].index == typeIndex) {
        type = StakeInstructionLayout[t];
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
}

/**
 * An enumeration of valid StakeInstructionTypes
 */
export const StakeInstructionLayout = Object.freeze({
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
  DelegateStake: {
    index: 2,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  RedeemVoteCredits: {
    index: 3,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  Split: {
    index: 4,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Withdraw: {
    index: 5,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Deactivate: {
    index: 6,
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
 * An enumeration of valid StakeInstructionTypes
 */
export const StakeAuthorizationLayout = Object.freeze({
  Staker: {
    index: 0,
  },
  Withdrawer: {
    index: 1,
  },
});

class RewardsPoolPublicKey extends PublicKey {
  static get rewardsPoolBaseId(): PublicKey {
    return new PublicKey('StakeRewards1111111111111111111111111111111');
  }

  /**
   * Generate Derive a public key from another key, a seed, and a programId.
   */
  static randomId(): PublicKey {
    const randomInt = Math.floor(Math.random() * (256 - 1));
    let pubkey = this.rewardsPoolBaseId;
    for (let i = 0; i < randomInt; i++) {
      const buffer = pubkey.toBuffer();
      const hash = hasha(buffer, {algorithm: 'sha256'});
      return new PublicKey('0x' + hash);
    }
    return pubkey;
  }
}

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
    return 2008;
  }

  /**
   * Generate an Initialize instruction to add to a Stake Create transaction
   */
  static initialize(
    stakeAccount: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
  ): TransactionInstruction {
    const type = StakeInstructionLayout.Initialize;
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
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
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
    stakeAccount: PublicKey,
    seed: string,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  ): Transaction {
    let transaction = SystemProgram.createAccountWithSeed(
      from,
      stakeAccount,
      seed,
      lamports,
      this.space,
      this.programId,
    );

    return transaction.add(this.initialize(stakeAccount, authorized, lockup));
  }

  /**
   * Generate a Transaction that creates a new Stake account
   */
  static createAccount(
    from: PublicKey,
    stakeAccount: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  ): Transaction {
    let transaction = SystemProgram.createAccount(
      from,
      stakeAccount,
      lamports,
      this.space,
      this.programId,
    );

    return transaction.add(this.initialize(stakeAccount, authorized, lockup));
  }

  /**
   * Generate a Transaction that delegates Stake tokens to a validator
   * Vote PublicKey. This transaction can also be used to redelegate Stake
   * to a new validator Vote PublicKey.
   */
  static delegate(
    stakeAccount: PublicKey,
    authorizedPubkey: PublicKey,
    votePubkey: PublicKey,
  ): Transaction {
    const type = StakeInstructionLayout.DelegateStake;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
        {pubkey: votePubkey, isSigner: false, isWritable: false},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
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
    stakeAccount: PublicKey,
    authorizedPubkey: PublicKey,
    newAuthorized: PublicKey,
    stakeAuthorizationType: StakeAuthorizationType,
  ): Transaction {
    const type = StakeInstructionLayout.Authorize;
    const data = encodeData(type, {
      newAuthorized: newAuthorized.toBuffer(),
      stakeAuthorizationType: stakeAuthorizationType.index,
    });

    return new Transaction().add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
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
  static redeemVoteCredits(
    stakeAccount: PublicKey,
    votePubkey: PublicKey,
  ): Transaction {
    const type = StakeInstructionLayout.RedeemVoteCredits;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
        {pubkey: votePubkey, isSigner: false, isWritable: true},
        {
          pubkey: RewardsPoolPublicKey.randomId(),
          isSigner: false,
          isWritable: true,
        },
        {pubkey: SYSVAR_REWARDS_PUBKEY, isSigner: false, isWritable: false},
        {
          pubkey: SYSVAR_STAKE_HISTORY_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that splits Stake tokens into another stake account
   */
  static split(
    stakeAccount: PublicKey,
    authorizedPubkey: PublicKey,
    lamports: number,
    splitStakePubkey: PublicKey,
  ): Transaction {
    let transaction = SystemProgram.createAccount(
      stakeAccount,
      splitStakePubkey,
      0,
      this.space,
      this.programId,
    );
    transaction.instructions[0].keys[0].isSigner = false;
    const type = StakeInstructionLayout.Split;
    const data = encodeData(type, {lamports});

    return transaction.add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
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
    stakeAccount: PublicKey,
    withdrawerPubkey: PublicKey,
    to: PublicKey,
    lamports: number,
  ): Transaction {
    const type = StakeInstructionLayout.Withdraw;
    const data = encodeData(type, {lamports});

    return new Transaction().add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
        {pubkey: to, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {
          pubkey: SYSVAR_STAKE_HISTORY_PUBKEY,
          isSigner: false,
          isWritable: false,
        },
        {pubkey: withdrawerPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that deactivates Stake tokens.
   */
  static deactivate(
    stakeAccount: PublicKey,
    authorizedPubkey: PublicKey,
  ): Transaction {
    const type = StakeInstructionLayout.Deactivate;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakeAccount, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }
}
