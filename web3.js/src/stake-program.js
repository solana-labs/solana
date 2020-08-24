// @flow

import * as BufferLayout from 'buffer-layout';

import {encodeData, decodeData} from './instruction';
import * as Layout from './layout';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_STAKE_HISTORY_PUBKEY,
} from './sysvar';
import {Transaction, TransactionInstruction} from './transaction';

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
 * Create stake account transaction params
 * @typedef {Object} CreateStakeAccountParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} stakePubkey
 * @property {Authorized} authorized
 * @property {Lockup} lockup
 * @property {number} lamports
 */
export type CreateStakeAccountParams = {|
  fromPubkey: PublicKey,
  stakePubkey: PublicKey,
  authorized: Authorized,
  lockup: Lockup,
  lamports: number,
|};

/**
 * Create stake account with seed transaction params
 * @typedef {Object} CreateStakeAccountWithSeedParams
 * @property {PublicKey} fromPubkey
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} basePubkey
 * @property {string} seed
 * @property {Authorized} authorized
 * @property {Lockup} lockup
 * @property {number} lamports
 */
export type CreateStakeAccountWithSeedParams = {|
  fromPubkey: PublicKey,
  stakePubkey: PublicKey,
  basePubkey: PublicKey,
  seed: string,
  authorized: Authorized,
  lockup: Lockup,
  lamports: number,
|};

/**
 * Initialize stake instruction params
 * @typedef {Object} InitializeStakeParams
 * @property {PublicKey} stakePubkey
 * @property {Authorized} authorized
 * @property {Lockup} lockup
 */
export type InitializeStakeParams = {|
  stakePubkey: PublicKey,
  authorized: Authorized,
  lockup: Lockup,
|};

/**
 * Delegate stake instruction params
 * @typedef {Object} DelegateStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} votePubkey
 */
export type DelegateStakeParams = {|
  stakePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  votePubkey: PublicKey,
|};

/**
 * Authorize stake instruction params
 * @typedef {Object} AuthorizeStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} newAuthorizedPubkey
 * @property {StakeAuthorizationType} stakeAuthorizationType
 */
export type AuthorizeStakeParams = {|
  stakePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  newAuthorizedPubkey: PublicKey,
  stakeAuthorizationType: StakeAuthorizationType,
|};

/**
 * Authorize stake instruction params using a derived key
 * @typedef {Object} AuthorizeWithSeedStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorityBase
 * @property {string} authoritySeed
 * @property {PublicKey} authorityOwner
 * @property {PublicKey} newAuthorizedPubkey
 * @property {StakeAuthorizationType} stakeAuthorizationType
 */
export type AuthorizeWithSeedStakeParams = {|
  stakePubkey: PublicKey,
  authorityBase: PublicKey,
  authoritySeed: string,
  authorityOwner: PublicKey,
  newAuthorizedPubkey: PublicKey,
  stakeAuthorizationType: StakeAuthorizationType,
|};

/**
 * Split stake instruction params
 * @typedef {Object} SplitStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} splitStakePubkey
 * @property {number} lamports
 */
export type SplitStakeParams = {|
  stakePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  splitStakePubkey: PublicKey,
  lamports: number,
|};

/**
 * Withdraw stake instruction params
 * @typedef {Object} WithdrawStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorizedPubkey
 * @property {PublicKey} toPubkey
 * @property {number} lamports
 */
export type WithdrawStakeParams = {|
  stakePubkey: PublicKey,
  authorizedPubkey: PublicKey,
  toPubkey: PublicKey,
  lamports: number,
|};

/**
 * Deactivate stake instruction params
 * @typedef {Object} DeactivateStakeParams
 * @property {PublicKey} stakePubkey
 * @property {PublicKey} authorizedPubkey
 */
export type DeactivateStakeParams = {|
  stakePubkey: PublicKey,
  authorizedPubkey: PublicKey,
|};

/**
 * Stake Instruction class
 */
export class StakeInstruction {
  /**
   * Decode a stake instruction and retrieve the instruction type.
   */
  static decodeInstructionType(
    instruction: TransactionInstruction,
  ): StakeInstructionType {
    this.checkProgramId(instruction.programId);

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

    return type;
  }

  /**
   * Decode a initialize stake instruction and retrieve the instruction params.
   */
  static decodeInitialize(
    instruction: TransactionInstruction,
  ): InitializeStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {authorized, lockup} = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.Initialize,
      instruction.data,
    );

    return {
      stakePubkey: instruction.keys[0].pubkey,
      authorized: new Authorized(
        new PublicKey(authorized.staker),
        new PublicKey(authorized.withdrawer),
      ),
      lockup: new Lockup(
        lockup.unixTimestamp,
        lockup.epoch,
        new PublicKey(lockup.custodian),
      ),
    };
  }

  /**
   * Decode a delegate stake instruction and retrieve the instruction params.
   */
  static decodeDelegate(
    instruction: TransactionInstruction,
  ): DelegateStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 6);
    decodeData(STAKE_INSTRUCTION_LAYOUTS.Delegate, instruction.data);

    return {
      stakePubkey: instruction.keys[0].pubkey,
      votePubkey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[5].pubkey,
    };
  }

  /**
   * Decode an authorize stake instruction and retrieve the instruction params.
   */
  static decodeAuthorize(
    instruction: TransactionInstruction,
  ): AuthorizeStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);
    const {newAuthorized, stakeAuthorizationType} = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.Authorize,
      instruction.data,
    );

    return {
      stakePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      stakeAuthorizationType: {
        index: stakeAuthorizationType,
      },
    };
  }

  /**
   * Decode an authorize-with-seed stake instruction and retrieve the instruction params.
   */
  static decodeAuthorizeWithSeed(
    instruction: TransactionInstruction,
  ): AuthorizeWithSeedStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);
    const {newAuthorized, stakeAuthorizationType, authoritySeed, authorityOwner} = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed,
      instruction.data,
    );

    return {
      stakePubkey: instruction.keys[0].pubkey,
      authorityBase: instruction.keys[1].pubkey,
      authoritySeed: authoritySeed,
      authorityOwner: new PublicKey(authorityOwner),
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      stakeAuthorizationType: {
        index: stakeAuthorizationType,
      },
    };
  }

  /**
   * Decode a split stake instruction and retrieve the instruction params.
   */
  static decodeSplit(instruction: TransactionInstruction): SplitStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);
    const {lamports} = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.Split,
      instruction.data,
    );

    return {
      stakePubkey: instruction.keys[0].pubkey,
      splitStakePubkey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
      lamports,
    };
  }

  /**
   * Decode a withdraw stake instruction and retrieve the instruction params.
   */
  static decodeWithdraw(
    instruction: TransactionInstruction,
  ): WithdrawStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 5);
    const {lamports} = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.Withdraw,
      instruction.data,
    );

    return {
      stakePubkey: instruction.keys[0].pubkey,
      toPubkey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[4].pubkey,
      lamports,
    };
  }

  /**
   * Decode a deactivate stake instruction and retrieve the instruction params.
   */
  static decodeDeactivate(
    instruction: TransactionInstruction,
  ): DeactivateStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);
    decodeData(STAKE_INSTRUCTION_LAYOUTS.Deactivate, instruction.data);

    return {
      stakePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
    };
  }

  /**
   * @private
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(StakeProgram.programId)) {
      throw new Error('invalid instruction; programId is not StakeProgram');
    }
  }

  /**
   * @private
   */
  static checkKeyLength(keys: Array<any>, expectedLength: number) {
    if (keys.length < expectedLength) {
      throw new Error(
        `invalid instruction; found ${keys.length} keys, expected at least ${expectedLength}`,
      );
    }
  }
}

/**
 * An enumeration of valid StakeInstructionType's
 * @typedef { 'Initialize' | 'Authorize' | 'AuthorizeWithSeed' | 'Delegate' | 'Split' | 'Withdraw'
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
  AuthorizeWithSeed: {
    index: 8,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('newAuthorized'),
      BufferLayout.u32('stakeAuthorizationType'),
      Layout.rustString('authoritySeed'),
      Layout.publicKey('authorityOwner'),
    ]),
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
  static initialize(params: InitializeStakeParams): TransactionInstruction {
    const {stakePubkey, authorized, lockup} = params;
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
    params: CreateStakeAccountWithSeedParams,
  ): Transaction {
    let transaction = SystemProgram.createAccountWithSeed({
      fromPubkey: params.fromPubkey,
      newAccountPubkey: params.stakePubkey,
      basePubkey: params.basePubkey,
      seed: params.seed,
      lamports: params.lamports,
      space: this.space,
      programId: this.programId,
    });

    const {stakePubkey, authorized, lockup} = params;
    return transaction.add(this.initialize({stakePubkey, authorized, lockup}));
  }

  /**
   * Generate a Transaction that creates a new Stake account
   */
  static createAccount(params: CreateStakeAccountParams): Transaction {
    let transaction = SystemProgram.createAccount({
      fromPubkey: params.fromPubkey,
      newAccountPubkey: params.stakePubkey,
      lamports: params.lamports,
      space: this.space,
      programId: this.programId,
    });

    const {stakePubkey, authorized, lockup} = params;
    return transaction.add(this.initialize({stakePubkey, authorized, lockup}));
  }

  /**
   * Generate a Transaction that delegates Stake tokens to a validator
   * Vote PublicKey. This transaction can also be used to redelegate Stake
   * to a new validator Vote PublicKey.
   */
  static delegate(params: DelegateStakeParams): Transaction {
    const {stakePubkey, authorizedPubkey, votePubkey} = params;

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
  static authorize(params: AuthorizeStakeParams): Transaction {
    const {
      stakePubkey,
      authorizedPubkey,
      newAuthorizedPubkey,
      stakeAuthorizationType,
    } = params;

    const type = STAKE_INSTRUCTION_LAYOUTS.Authorize;
    const data = encodeData(type, {
      newAuthorized: newAuthorizedPubkey.toBuffer(),
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
   * Generate a Transaction that authorizes a new PublicKey as Staker
   * or Withdrawer on the Stake account.
   */
  static authorizeWithSeed(params: AuthorizeWithSeedStakeParams): Transaction {
    const {
      stakePubkey,
      authorityBase,
      authoritySeed,
      authorityOwner,
      newAuthorizedPubkey,
      stakeAuthorizationType,
    } = params;

    const type = STAKE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed;
    const data = encodeData(type, {
      newAuthorized: newAuthorizedPubkey.toBuffer(),
      stakeAuthorizationType: stakeAuthorizationType.index,
      authoritySeed: authoritySeed,
      authorityOwner: authorityOwner.toBuffer(),
    });

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: authorityBase, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that splits Stake tokens into another stake account
   */
  static split(params: SplitStakeParams): Transaction {
    const {stakePubkey, authorizedPubkey, splitStakePubkey, lamports} = params;

    let transaction = SystemProgram.createAccount({
      fromPubkey: authorizedPubkey,
      newAccountPubkey: splitStakePubkey,
      lamports: 0,
      space: this.space,
      programId: this.programId,
    });
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
  static withdraw(params: WithdrawStakeParams): Transaction {
    const {stakePubkey, authorizedPubkey, toPubkey, lamports} = params;
    const type = STAKE_INSTRUCTION_LAYOUTS.Withdraw;
    const data = encodeData(type, {lamports});

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: toPubkey, isSigner: false, isWritable: true},
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
  static deactivate(params: DeactivateStakeParams): Transaction {
    const {stakePubkey, authorizedPubkey} = params;
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
