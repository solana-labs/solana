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
import {SYSVAR_CLOCK_PUBKEY, SYSVAR_RENT_PUBKEY} from '../sysvar';
import {Transaction, TransactionInstruction} from '../transaction';
import {toBuffer} from '../utils/to-buffer';

/**
 * Vote account info
 */
export class VoteInit {
  nodePubkey: PublicKey;
  authorizedVoter: PublicKey;
  authorizedWithdrawer: PublicKey;
  commission: number; /** [0, 100] */

  constructor(
    nodePubkey: PublicKey,
    authorizedVoter: PublicKey,
    authorizedWithdrawer: PublicKey,
    commission: number,
  ) {
    this.nodePubkey = nodePubkey;
    this.authorizedVoter = authorizedVoter;
    this.authorizedWithdrawer = authorizedWithdrawer;
    this.commission = commission;
  }
}

/**
 * Create vote account transaction params
 */
export type CreateVoteAccountParams = {
  fromPubkey: PublicKey;
  votePubkey: PublicKey;
  voteInit: VoteInit;
  lamports: number;
};

/**
 * InitializeAccount instruction params
 */
export type InitializeAccountParams = {
  votePubkey: PublicKey;
  nodePubkey: PublicKey;
  voteInit: VoteInit;
};

/**
 * Authorize instruction params
 */
export type AuthorizeVoteParams = {
  votePubkey: PublicKey;
  /** Current vote or withdraw authority, depending on `voteAuthorizationType` */
  authorizedPubkey: PublicKey;
  newAuthorizedPubkey: PublicKey;
  voteAuthorizationType: VoteAuthorizationType;
};

/**
 * AuthorizeWithSeed instruction params
 */
export type AuthorizeVoteWithSeedParams = {
  currentAuthorityDerivedKeyBasePubkey: PublicKey;
  currentAuthorityDerivedKeyOwnerPubkey: PublicKey;
  currentAuthorityDerivedKeySeed: string;
  newAuthorizedPubkey: PublicKey;
  voteAuthorizationType: VoteAuthorizationType;
  votePubkey: PublicKey;
};

/**
 * Withdraw from vote account transaction params
 */
export type WithdrawFromVoteAccountParams = {
  votePubkey: PublicKey;
  authorizedWithdrawerPubkey: PublicKey;
  lamports: number;
  toPubkey: PublicKey;
};

/**
 * Vote Instruction class
 */
export class VoteInstruction {
  /**
   * @internal
   */
  constructor() {}

  /**
   * Decode a vote instruction and retrieve the instruction type.
   */
  static decodeInstructionType(
    instruction: TransactionInstruction,
  ): VoteInstructionType {
    this.checkProgramId(instruction.programId);

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const typeIndex = instructionTypeLayout.decode(instruction.data);

    let type: VoteInstructionType | undefined;
    for (const [ixType, layout] of Object.entries(VOTE_INSTRUCTION_LAYOUTS)) {
      if (layout.index == typeIndex) {
        type = ixType as VoteInstructionType;
        break;
      }
    }

    if (!type) {
      throw new Error('Instruction type incorrect; not a VoteInstruction');
    }

    return type;
  }

  /**
   * Decode an initialize vote instruction and retrieve the instruction params.
   */
  static decodeInitializeAccount(
    instruction: TransactionInstruction,
  ): InitializeAccountParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 4);

    const {voteInit} = decodeData(
      VOTE_INSTRUCTION_LAYOUTS.InitializeAccount,
      instruction.data,
    );

    return {
      votePubkey: instruction.keys[0].pubkey,
      nodePubkey: instruction.keys[3].pubkey,
      voteInit: new VoteInit(
        new PublicKey(voteInit.nodePubkey),
        new PublicKey(voteInit.authorizedVoter),
        new PublicKey(voteInit.authorizedWithdrawer),
        voteInit.commission,
      ),
    };
  }

  /**
   * Decode an authorize instruction and retrieve the instruction params.
   */
  static decodeAuthorize(
    instruction: TransactionInstruction,
  ): AuthorizeVoteParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);

    const {newAuthorized, voteAuthorizationType} = decodeData(
      VOTE_INSTRUCTION_LAYOUTS.Authorize,
      instruction.data,
    );

    return {
      votePubkey: instruction.keys[0].pubkey,
      authorizedPubkey: instruction.keys[2].pubkey,
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      voteAuthorizationType: {
        index: voteAuthorizationType,
      },
    };
  }

  /**
   * Decode an authorize instruction and retrieve the instruction params.
   */
  static decodeAuthorizeWithSeed(
    instruction: TransactionInstruction,
  ): AuthorizeVoteWithSeedParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);

    const {
      voteAuthorizeWithSeedArgs: {
        currentAuthorityDerivedKeyOwnerPubkey,
        currentAuthorityDerivedKeySeed,
        newAuthorized,
        voteAuthorizationType,
      },
    } = decodeData(
      VOTE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed,
      instruction.data,
    );

    return {
      currentAuthorityDerivedKeyBasePubkey: instruction.keys[2].pubkey,
      currentAuthorityDerivedKeyOwnerPubkey: new PublicKey(
        currentAuthorityDerivedKeyOwnerPubkey,
      ),
      currentAuthorityDerivedKeySeed: currentAuthorityDerivedKeySeed,
      newAuthorizedPubkey: new PublicKey(newAuthorized),
      voteAuthorizationType: {
        index: voteAuthorizationType,
      },
      votePubkey: instruction.keys[0].pubkey,
    };
  }

  /**
   * Decode a withdraw instruction and retrieve the instruction params.
   */
  static decodeWithdraw(
    instruction: TransactionInstruction,
  ): WithdrawFromVoteAccountParams {
    this.checkProgramId(instruction.programId);
    this.checkKeyLength(instruction.keys, 3);

    const {lamports} = decodeData(
      VOTE_INSTRUCTION_LAYOUTS.Withdraw,
      instruction.data,
    );

    return {
      votePubkey: instruction.keys[0].pubkey,
      authorizedWithdrawerPubkey: instruction.keys[2].pubkey,
      lamports,
      toPubkey: instruction.keys[1].pubkey,
    };
  }

  /**
   * @internal
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(VoteProgram.programId)) {
      throw new Error('invalid instruction; programId is not VoteProgram');
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
 * An enumeration of valid VoteInstructionType's
 */
export type VoteInstructionType =
  // FIXME
  // It would be preferable for this type to be `keyof VoteInstructionInputData`
  // but Typedoc does not transpile `keyof` expressions.
  // See https://github.com/TypeStrong/typedoc/issues/1894
  'Authorize' | 'AuthorizeWithSeed' | 'InitializeAccount' | 'Withdraw';

/** @internal */
export type VoteAuthorizeWithSeedArgs = Readonly<{
  currentAuthorityDerivedKeyOwnerPubkey: Uint8Array;
  currentAuthorityDerivedKeySeed: string;
  newAuthorized: Uint8Array;
  voteAuthorizationType: number;
}>;
type VoteInstructionInputData = {
  Authorize: IInstructionInputData & {
    newAuthorized: Uint8Array;
    voteAuthorizationType: number;
  };
  AuthorizeWithSeed: IInstructionInputData & {
    voteAuthorizeWithSeedArgs: VoteAuthorizeWithSeedArgs;
  };
  InitializeAccount: IInstructionInputData & {
    voteInit: Readonly<{
      authorizedVoter: Uint8Array;
      authorizedWithdrawer: Uint8Array;
      commission: number;
      nodePubkey: Uint8Array;
    }>;
  };
  Withdraw: IInstructionInputData & {
    lamports: number;
  };
};

const VOTE_INSTRUCTION_LAYOUTS = Object.freeze<{
  [Instruction in VoteInstructionType]: InstructionType<
    VoteInstructionInputData[Instruction]
  >;
}>({
  InitializeAccount: {
    index: 0,
    layout: BufferLayout.struct<VoteInstructionInputData['InitializeAccount']>([
      BufferLayout.u32('instruction'),
      Layout.voteInit(),
    ]),
  },
  Authorize: {
    index: 1,
    layout: BufferLayout.struct<VoteInstructionInputData['Authorize']>([
      BufferLayout.u32('instruction'),
      Layout.publicKey('newAuthorized'),
      BufferLayout.u32('voteAuthorizationType'),
    ]),
  },
  Withdraw: {
    index: 3,
    layout: BufferLayout.struct<VoteInstructionInputData['Withdraw']>([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
    ]),
  },
  AuthorizeWithSeed: {
    index: 10,
    layout: BufferLayout.struct<VoteInstructionInputData['AuthorizeWithSeed']>([
      BufferLayout.u32('instruction'),
      Layout.voteAuthorizeWithSeedArgs(),
    ]),
  },
});

/**
 * VoteAuthorize type
 */
export type VoteAuthorizationType = {
  /** The VoteAuthorize index (from solana-vote-program) */
  index: number;
};

/**
 * An enumeration of valid VoteAuthorization layouts.
 */
export const VoteAuthorizationLayout = Object.freeze({
  Voter: {
    index: 0,
  },
  Withdrawer: {
    index: 1,
  },
});

/**
 * Factory class for transactions to interact with the Vote program
 */
export class VoteProgram {
  /**
   * @internal
   */
  constructor() {}

  /**
   * Public key that identifies the Vote program
   */
  static programId: PublicKey = new PublicKey(
    'Vote111111111111111111111111111111111111111',
  );

  /**
   * Max space of a Vote account
   *
   * This is generated from the solana-vote-program VoteState struct as
   * `VoteState::size_of()`:
   * https://docs.rs/solana-vote-program/1.9.5/solana_vote_program/vote_state/struct.VoteState.html#method.size_of
   */
  static space: number = 3731;

  /**
   * Generate an Initialize instruction.
   */
  static initializeAccount(
    params: InitializeAccountParams,
  ): TransactionInstruction {
    const {votePubkey, nodePubkey, voteInit} = params;
    const type = VOTE_INSTRUCTION_LAYOUTS.InitializeAccount;
    const data = encodeData(type, {
      voteInit: {
        nodePubkey: toBuffer(voteInit.nodePubkey.toBuffer()),
        authorizedVoter: toBuffer(voteInit.authorizedVoter.toBuffer()),
        authorizedWithdrawer: toBuffer(
          voteInit.authorizedWithdrawer.toBuffer(),
        ),
        commission: voteInit.commission,
      },
    });
    const instructionData = {
      keys: [
        {pubkey: votePubkey, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: nodePubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    };
    return new TransactionInstruction(instructionData);
  }

  /**
   * Generate a transaction that creates a new Vote account.
   */
  static createAccount(params: CreateVoteAccountParams): Transaction {
    const transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccount({
        fromPubkey: params.fromPubkey,
        newAccountPubkey: params.votePubkey,
        lamports: params.lamports,
        space: this.space,
        programId: this.programId,
      }),
    );

    return transaction.add(
      this.initializeAccount({
        votePubkey: params.votePubkey,
        nodePubkey: params.voteInit.nodePubkey,
        voteInit: params.voteInit,
      }),
    );
  }

  /**
   * Generate a transaction that authorizes a new Voter or Withdrawer on the Vote account.
   */
  static authorize(params: AuthorizeVoteParams): Transaction {
    const {
      votePubkey,
      authorizedPubkey,
      newAuthorizedPubkey,
      voteAuthorizationType,
    } = params;

    const type = VOTE_INSTRUCTION_LAYOUTS.Authorize;
    const data = encodeData(type, {
      newAuthorized: toBuffer(newAuthorizedPubkey.toBuffer()),
      voteAuthorizationType: voteAuthorizationType.index,
    });

    const keys = [
      {pubkey: votePubkey, isSigner: false, isWritable: true},
      {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
      {pubkey: authorizedPubkey, isSigner: true, isWritable: false},
    ];

    return new Transaction().add({
      keys,
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction that authorizes a new Voter or Withdrawer on the Vote account
   * where the current Voter or Withdrawer authority is a derived key.
   */
  static authorizeWithSeed(params: AuthorizeVoteWithSeedParams): Transaction {
    const {
      currentAuthorityDerivedKeyBasePubkey,
      currentAuthorityDerivedKeyOwnerPubkey,
      currentAuthorityDerivedKeySeed,
      newAuthorizedPubkey,
      voteAuthorizationType,
      votePubkey,
    } = params;

    const type = VOTE_INSTRUCTION_LAYOUTS.AuthorizeWithSeed;
    const data = encodeData(type, {
      voteAuthorizeWithSeedArgs: {
        currentAuthorityDerivedKeyOwnerPubkey: toBuffer(
          currentAuthorityDerivedKeyOwnerPubkey.toBuffer(),
        ),
        currentAuthorityDerivedKeySeed: currentAuthorityDerivedKeySeed,
        newAuthorized: toBuffer(newAuthorizedPubkey.toBuffer()),
        voteAuthorizationType: voteAuthorizationType.index,
      },
    });

    const keys = [
      {pubkey: votePubkey, isSigner: false, isWritable: true},
      {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
      {
        pubkey: currentAuthorityDerivedKeyBasePubkey,
        isSigner: true,
        isWritable: false,
      },
    ];

    return new Transaction().add({
      keys,
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction to withdraw from a Vote account.
   */
  static withdraw(params: WithdrawFromVoteAccountParams): Transaction {
    const {votePubkey, authorizedWithdrawerPubkey, lamports, toPubkey} = params;
    const type = VOTE_INSTRUCTION_LAYOUTS.Withdraw;
    const data = encodeData(type, {lamports});

    const keys = [
      {pubkey: votePubkey, isSigner: false, isWritable: true},
      {pubkey: toPubkey, isSigner: false, isWritable: true},
      {pubkey: authorizedWithdrawerPubkey, isSigner: true, isWritable: false},
    ];

    return new Transaction().add({
      keys,
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction to withdraw safely from a Vote account.
   *
   * This function was created as a safeguard for vote accounts running validators, `safeWithdraw`
   * checks that the withdraw amount will not exceed the specified balance while leaving enough left
   * to cover rent. If you wish to close the vote account by withdrawing the full amount, call the
   * `withdraw` method directly.
   */
  static safeWithdraw(
    params: WithdrawFromVoteAccountParams,
    currentVoteAccountBalance: number,
    rentExemptMinimum: number,
  ): Transaction {
    if (params.lamports > currentVoteAccountBalance - rentExemptMinimum) {
      throw new Error(
        'Withdraw will leave vote account with insuffcient funds.',
      );
    }
    return VoteProgram.withdraw(params);
  }
}
