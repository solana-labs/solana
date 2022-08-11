import * as BufferLayout from '@solana/buffer-layout';

import {
  encodeData,
  decodeData,
  InstructionType,
  IInstructionInputData,
} from '../instruction';
import * as Layout from '../layout';
import {PublicKey} from '../publickey';
import {SystemProgram} from './system';
import {
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_STAKE_HISTORY_PUBKEY,
} from '../sysvar';
import {Transaction, TransactionInstruction} from '../transaction';
import {toBuffer} from '../utils/to-buffer';

/**
 * Address of the stake config account which configures the rate
 * of stake warmup and cooldown as well as the slashing penalty.
 */
export const STAKE_CONFIG_ID = new PublicKey(
  'StakeConfig11111111111111111111111111111111',
);

/**
 * Stake account authority info
 */
export class Authorized {
  /** stake authority */
  staker: PublicKey;
  /** withdraw authority */
  withdrawer: PublicKey;

  /**
   * Create a new Authorized object
   * @param staker the stake authority
   * @param withdrawer the withdraw authority
   */
  constructor(staker: PublicKey, withdrawer: PublicKey) {
    this.staker = staker;
    this.withdrawer = withdrawer;
  }
}

type AuthorizedRaw = Readonly<{
  staker: Uint8Array;
  withdrawer: Uint8Array;
}>;

/**
 * Stake account lockup info
 */
export class Lockup {
  /** Unix timestamp of lockup expiration */
  unixTimestamp: number;
  /** Epoch of lockup expiration */
  epoch: number;
  /** Lockup custodian authority */
  custodian: PublicKey;

  /**
   * Create a new Lockup object
   */
  constructor(unixTimestamp: number, epoch: number, custodian: PublicKey) {
    this.unixTimestamp = unixTimestamp;
    this.epoch = epoch;
    this.custodian = custodian;
  }

  /**
   * Default, inactive Lockup value
   */
  static default: Lockup = new Lockup(0, 0, PublicKey.default);
}

type LockupRaw = Readonly<{
  custodian: Uint8Array;
  epoch: number;
  unixTimestamp: number;
}>;

/**
 * Create stake account transaction params
 */
export type CreateStakeAccountParams = {
  /** Address of the account which will fund creation */
  fromPubkey: PublicKey;
  /** Address of the new stake account */
  stakePubkey: PublicKey;
  /** Authorities of the new stake account */
  authorized: Authorized;
  /** Lockup of the new stake account */
  lockup?: Lockup;
  /** Funding amount */
  lamports: number;
};

/**
 * Create stake account with seed transaction params
 */
export type CreateStakeAccountWithSeedParams = {
  fromPubkey: PublicKey;
  stakePubkey: PublicKey;
  basePubkey: PublicKey;
  seed: string;
  authorized: Authorized;
  lockup?: Lockup;
  lamports: number;
};

/**
 * Initialize stake instruction params
 */
export type InitializeStakeParams = {
  stakePubkey: PublicKey;
  authorized: Authorized;
  lockup?: Lockup;
};

/**
 * Delegate stake instruction params
 */
export type DelegateStakeParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
  votePubkey: PublicKey;
};

/**
 * Authorize stake instruction params
 */
export type AuthorizeStakeParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
  newAuthorizedPubkey: PublicKey;
  stakeAuthorizationType: StakeAuthorizationType;
  custodianPubkey?: PublicKey;
};

/**
 * Authorize stake instruction params using a derived key
 */
export type AuthorizeWithSeedStakeParams = {
  stakePubkey: PublicKey;
  authorityBase: PublicKey;
  authoritySeed: string;
  authorityOwner: PublicKey;
  newAuthorizedPubkey: PublicKey;
  stakeAuthorizationType: StakeAuthorizationType;
  custodianPubkey?: PublicKey;
};

/**
 * Split stake instruction params
 */
export type SplitStakeParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
  splitStakePubkey: PublicKey;
  lamports: number;
};

/**
 * Split with seed transaction params
 */
export type SplitStakeWithSeedParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
  splitStakePubkey: PublicKey;
  basePubkey: PublicKey;
  seed: string;
  lamports: number;
};

/**
 * Withdraw stake instruction params
 */
export type WithdrawStakeParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
  toPubkey: PublicKey;
  lamports: number;
  custodianPubkey?: PublicKey;
};

/**
 * Deactivate stake instruction params
 */
export type DeactivateStakeParams = {
  stakePubkey: PublicKey;
  authorizedPubkey: PublicKey;
};

/**
 * Merge stake instruction params
 */
export type MergeStakeParams = {
  stakePubkey: PublicKey;
  sourceStakePubKey: PublicKey;
  authorizedPubkey: PublicKey;
};

/**
 * Stake Instruction class
 */
export class StakeInstruction {
  /**
   * @internal
   */
  constructor() {}

  /**
   * Decode a stake instruction and retrieve the instruction type.
   */
  static decodeInstructionType(
    instruction: TransactionInstruction,
  ): StakeInstructionType {
    this.checkProgramId(instruction.programId);

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);

    let type: StakeInstructionType | undefined;
    for (const [ixType, layout] of Object.entries(STAKE_INSTRUCTION_LAYOUTS)) {
      if (layout.index == typeIndex) {
        type = ixType as StakeInstructionType;
        break;
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

    const o: AuthorizeStakeParams = {
      stakePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      stakeAuthorizationType: {
        index: stakeAuthorizationType,
      },
    };
    if (instruction.keys.length > 3) {
      o.custodianPubkey = instruction.keys[3].pubkey;
    }
    return o;
  }

  /**
   * Decode an authorize-with-seed stake instruction and retrieve the instruction params.
   */
  static decodeAuthorizeWithSeed(
    instruction: TransactionInstruction,
  ): AuthorizeWithSeedStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 2);

    const {
      newAuthorized,
      stakeAuthorizationType,
      authoritySeed,
      authorityOwner,
    } = decodeData(
      STAKE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed,
      instruction.data,
    );

    const o: AuthorizeWithSeedStakeParams = {
      stakePubkey: instruction.keys[0].pubkey,
      authorityBase: instruction.keys[1].pubkey,
      authoritySeed: authoritySeed,
      authorityOwner: new PublicKey(authorityOwner),
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      stakeAuthorizationType: {
        index: stakeAuthorizationType,
      },
    };
    if (instruction.keys.length > 3) {
      o.custodianPubkey = instruction.keys[3].pubkey;
    }
    return o;
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
   * Decode a merge stake instruction and retrieve the instruction params.
   */
  static decodeMerge(instruction: TransactionInstruction): MergeStakeParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);
    decodeData(STAKE_INSTRUCTION_LAYOUTS.Merge, instruction.data);

    return {
      stakePubkey: instruction.keys[0].pubkey,
      sourceStakePubKey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[4].pubkey,
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

    const o: WithdrawStakeParams = {
      stakePubkey: instruction.keys[0].pubkey,
      toPubkey: instruction.keys[1].pubkey,
      authorizedPubkey: instruction.keys[4].pubkey,
      lamports,
    };
    if (instruction.keys.length > 5) {
      o.custodianPubkey = instruction.keys[5].pubkey;
    }
    return o;
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
   * @internal
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(StakeProgram.programId)) {
      throw new Error('invalid instruction; programId is not StakeProgram');
    }
  }

  /**
   * @internal
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
 */
export type StakeInstructionType =
  // FIXME
  // It would be preferable for this type to be `keyof StakeInstructionInputData`
  // but Typedoc does not transpile `keyof` expressions.
  // See https://github.com/TypeStrong/typedoc/issues/1894
  | 'Authorize'
  | 'AuthorizeWithSeed'
  | 'Deactivate'
  | 'Delegate'
  | 'Initialize'
  | 'Merge'
  | 'Split'
  | 'Withdraw';

type StakeInstructionInputData = {
  Authorize: IInstructionInputData &
    Readonly<{
      newAuthorized: Uint8Array;
      stakeAuthorizationType: number;
    }>;
  AuthorizeWithSeed: IInstructionInputData &
    Readonly<{
      authorityOwner: Uint8Array;
      authoritySeed: string;
      instruction: number;
      newAuthorized: Uint8Array;
      stakeAuthorizationType: number;
    }>;
  Deactivate: IInstructionInputData;
  Delegate: IInstructionInputData;
  Initialize: IInstructionInputData &
    Readonly<{
      authorized: AuthorizedRaw;
      lockup: LockupRaw;
    }>;
  Merge: IInstructionInputData;
  Split: IInstructionInputData &
    Readonly<{
      lamports: number;
    }>;
  Withdraw: IInstructionInputData &
    Readonly<{
      lamports: number;
    }>;
};

/**
 * An enumeration of valid stake InstructionType's
 * @internal
 */
export const STAKE_INSTRUCTION_LAYOUTS = Object.freeze<{
  [Instruction in StakeInstructionType]: InstructionType<
    StakeInstructionInputData[Instruction]
  >;
}>({
  Initialize: {
    index: 0,
    layout: BufferLayout.struct<StakeInstructionInputData['Initialize']>([
      BufferLayout.u32('instruction'),
      Layout.authorized(),
      Layout.lockup(),
    ]),
  },
  Authorize: {
    index: 1,
    layout: BufferLayout.struct<StakeInstructionInputData['Authorize']>([
      BufferLayout.u32('instruction'),
      Layout.publicKey('newAuthorized'),
      BufferLayout.u32('stakeAuthorizationType'),
    ]),
  },
  Delegate: {
    index: 2,
    layout: BufferLayout.struct<StakeInstructionInputData['Delegate']>([
      BufferLayout.u32('instruction'),
    ]),
  },
  Split: {
    index: 3,
    layout: BufferLayout.struct<StakeInstructionInputData['Split']>([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Withdraw: {
    index: 4,
    layout: BufferLayout.struct<StakeInstructionInputData['Withdraw']>([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  Deactivate: {
    index: 5,
    layout: BufferLayout.struct<StakeInstructionInputData['Deactivate']>([
      BufferLayout.u32('instruction'),
    ]),
  },
  Merge: {
    index: 7,
    layout: BufferLayout.struct<StakeInstructionInputData['Merge']>([
      BufferLayout.u32('instruction'),
    ]),
  },
  AuthorizeWithSeed: {
    index: 8,
    layout: BufferLayout.struct<StakeInstructionInputData['AuthorizeWithSeed']>(
      [
        BufferLayout.u32('instruction'),
        Layout.publicKey('newAuthorized'),
        BufferLayout.u32('stakeAuthorizationType'),
        Layout.rustString('authoritySeed'),
        Layout.publicKey('authorityOwner'),
      ],
    ),
  },
});

/**
 * Stake authorization type
 */
export type StakeAuthorizationType = {
  /** The Stake Authorization index (from solana-stake-program) */
  index: number;
};

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
   * @internal
   */
  constructor() {}

  /**
   * Public key that identifies the Stake program
   */
  static programId: PublicKey = new PublicKey(
    'Stake11111111111111111111111111111111111111',
  );

  /**
   * Max space of a Stake account
   *
   * This is generated from the solana-stake-program StakeState struct as
   * `StakeState::size_of()`:
   * https://docs.rs/solana-stake-program/latest/solana_stake_program/stake_state/enum.StakeState.html
   */
  static space: number = 200;

  /**
   * Generate an Initialize instruction to add to a Stake Create transaction
   */
  static initialize(params: InitializeStakeParams): TransactionInstruction {
    const {stakePubkey, authorized, lockup: maybeLockup} = params;
    const lockup: Lockup = maybeLockup || Lockup.default;
    const type = STAKE_INSTRUCTION_LAYOUTS.Initialize;
    const data = encodeData(type, {
      authorized: {
        staker: toBuffer(authorized.staker.toBuffer()),
        withdrawer: toBuffer(authorized.withdrawer.toBuffer()),
      },
      lockup: {
        unixTimestamp: lockup.unixTimestamp,
        epoch: lockup.epoch,
        custodian: toBuffer(lockup.custodian.toBuffer()),
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
    const transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccountWithSeed({
        fromPubkey: params.fromPubkey,
        newAccountPubkey: params.stakePubkey,
        basePubkey: params.basePubkey,
        seed: params.seed,
        lamports: params.lamports,
        space: this.space,
        programId: this.programId,
      }),
    );

    const {stakePubkey, authorized, lockup} = params;
    return transaction.add(this.initialize({stakePubkey, authorized, lockup}));
  }

  /**
   * Generate a Transaction that creates a new Stake account
   */
  static createAccount(params: CreateStakeAccountParams): Transaction {
    const transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccount({
        fromPubkey: params.fromPubkey,
        newAccountPubkey: params.stakePubkey,
        lamports: params.lamports,
        space: this.space,
        programId: this.programId,
      }),
    );

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
      custodianPubkey,
    } = params;

    const type = STAKE_INSTRUCTION_LAYOUTS.Authorize;
    const data = encodeData(type, {
      newAuthorized: toBuffer(newAuthorizedPubkey.toBuffer()),
      stakeAuthorizationType: stakeAuthorizationType.index,
    });

    const keys = [
      {pubkey: stakePubkey, isSigner: false, isWritable: true},
      {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: true},
      {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
    ];
    if (custodianPubkey) {
      keys.push({pubkey: custodianPubkey, isSigner: false, isWritable: false});
    }
    return new Transaction().add({
      keys,
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
      custodianPubkey,
    } = params;

    const type = STAKE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed;
    const data = encodeData(type, {
      newAuthorized: toBuffer(newAuthorizedPubkey.toBuffer()),
      stakeAuthorizationType: stakeAuthorizationType.index,
      authoritySeed: authoritySeed,
      authorityOwner: toBuffer(authorityOwner.toBuffer()),
    });

    const keys = [
      {pubkey: stakePubkey, isSigner: false, isWritable: true},
      {pubkey: authorityBase, isSigner: true, isWritable: false},
      {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
    ];
    if (custodianPubkey) {
      keys.push({pubkey: custodianPubkey, isSigner: false, isWritable: false});
    }
    return new Transaction().add({
      keys,
      programId: this.programId,
      data,
    });
  }

  /**
   * @internal
   */
  static splitInstruction(params: SplitStakeParams): TransactionInstruction {
    const {stakePubkey, authorizedPubkey, splitStakePubkey, lamports} = params;
    const type = STAKE_INSTRUCTION_LAYOUTS.Split;
    const data = encodeData(type, {lamports});
    return new TransactionInstruction({
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
   * Generate a Transaction that splits Stake tokens into another stake account
   */
  static split(params: SplitStakeParams): Transaction {
    const transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccount({
        fromPubkey: params.authorizedPubkey,
        newAccountPubkey: params.splitStakePubkey,
        lamports: 0,
        space: this.space,
        programId: this.programId,
      }),
    );
    return transaction.add(this.splitInstruction(params));
  }

  /**
   * Generate a Transaction that splits Stake tokens into another account
   * derived from a base public key and seed
   */
  static splitWithSeed(params: SplitStakeWithSeedParams): Transaction {
    const {
      stakePubkey,
      authorizedPubkey,
      splitStakePubkey,
      basePubkey,
      seed,
      lamports,
    } = params;
    const transaction = new Transaction();
    transaction.add(
      SystemProgram.allocate({
        accountPubkey: splitStakePubkey,
        basePubkey,
        seed,
        space: this.space,
        programId: this.programId,
      }),
    );
    return transaction.add(
      this.splitInstruction({
        stakePubkey,
        authorizedPubkey,
        splitStakePubkey,
        lamports,
      }),
    );
  }

  /**
   * Generate a Transaction that merges Stake accounts.
   */
  static merge(params: MergeStakeParams): Transaction {
    const {stakePubkey, sourceStakePubKey, authorizedPubkey} = params;
    const type = STAKE_INSTRUCTION_LAYOUTS.Merge;
    const data = encodeData(type);

    return new Transaction().add({
      keys: [
        {pubkey: stakePubkey, isSigner: false, isWritable: true},
        {pubkey: sourceStakePubKey, isSigner: false, isWritable: true},
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
   * Generate a Transaction that withdraws deactivated Stake tokens.
   */
  static withdraw(params: WithdrawStakeParams): Transaction {
    const {stakePubkey, authorizedPubkey, toPubkey, lamports, custodianPubkey} =
      params;
    const type = STAKE_INSTRUCTION_LAYOUTS.Withdraw;
    const data = encodeData(type, {lamports});

    const keys = [
      {pubkey: stakePubkey, isSigner: false, isWritable: true},
      {pubkey: toPubkey, isSigner: false, isWritable: true},
      {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
      {
        pubkey: SYSVAR_STAKE_HISTORY_PUBKEY,
        isSigner: false,
        isWritable: false,
      },
      {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
    ];
    if (custodianPubkey) {
      keys.push({pubkey: custodianPubkey, isSigner: false, isWritable: false});
    }
    return new Transaction().add({
      keys,
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
