import {toBufferLE} from 'bigint-buffer';
import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from '../../layout';
import {PublicKey} from '../../publickey';
import * as bigintLayout from '../../utils/bigint';
import {SystemProgram} from '../system';
import {TransactionInstruction} from '../../transaction';
import {decodeData, encodeData, IInstructionInputData} from '../../instruction';

export * from './state';

export type CreateLookupTableParams = {
  /** Account used to derive and control the new address lookup table. */
  authority: PublicKey;
  /** Account that will fund the new address lookup table. */
  payer: PublicKey;
  /** A recent slot must be used in the derivation path for each initialized table. */
  recentSlot: bigint | number;
};

export type FreezeLookupTableParams = {
  /** Address lookup table account to freeze. */
  lookupTable: PublicKey;
  /** Account which is the current authority. */
  authority: PublicKey;
};

export type ExtendLookupTableParams = {
  /** Address lookup table account to extend. */
  lookupTable: PublicKey;
  /** Account which is the current authority. */
  authority: PublicKey;
  /** Account that will fund the table reallocation.
   * Not required if the reallocation has already been funded. */
  payer?: PublicKey;
  /** List of Public Keys to be added to the lookup table. */
  addresses: Array<PublicKey>;
};

export type DeactivateLookupTableParams = {
  /** Address lookup table account to deactivate. */
  lookupTable: PublicKey;
  /** Account which is the current authority. */
  authority: PublicKey;
};

export type CloseLookupTableParams = {
  /** Address lookup table account to close. */
  lookupTable: PublicKey;
  /** Account which is the current authority. */
  authority: PublicKey;
  /** Recipient of closed account lamports. */
  recipient: PublicKey;
};

/**
 * An enumeration of valid LookupTableInstructionType's
 */
export type LookupTableInstructionType =
  | 'CreateLookupTable'
  | 'ExtendLookupTable'
  | 'CloseLookupTable'
  | 'FreezeLookupTable'
  | 'DeactivateLookupTable';

type LookupTableInstructionInputData = {
  CreateLookupTable: IInstructionInputData &
    Readonly<{
      recentSlot: bigint;
      bumpSeed: number;
    }>;
  FreezeLookupTable: IInstructionInputData;
  ExtendLookupTable: IInstructionInputData &
    Readonly<{
      numberOfAddresses: bigint;
      addresses: Array<Uint8Array>;
    }>;
  DeactivateLookupTable: IInstructionInputData;
  CloseLookupTable: IInstructionInputData;
};

/**
 * An enumeration of valid address lookup table InstructionType's
 * @internal
 */
export const LOOKUP_TABLE_INSTRUCTION_LAYOUTS = Object.freeze({
  CreateLookupTable: {
    index: 0,
    layout: BufferLayout.struct<
      LookupTableInstructionInputData['CreateLookupTable']
    >([
      BufferLayout.u32('instruction'),
      bigintLayout.u64('recentSlot'),
      BufferLayout.u8('bumpSeed'),
    ]),
  },
  FreezeLookupTable: {
    index: 1,
    layout: BufferLayout.struct<
      LookupTableInstructionInputData['FreezeLookupTable']
    >([BufferLayout.u32('instruction')]),
  },
  ExtendLookupTable: {
    index: 2,
    layout: BufferLayout.struct<
      LookupTableInstructionInputData['ExtendLookupTable']
    >([
      BufferLayout.u32('instruction'),
      bigintLayout.u64(),
      BufferLayout.seq(
        Layout.publicKey(),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'addresses',
      ),
    ]),
  },
  DeactivateLookupTable: {
    index: 3,
    layout: BufferLayout.struct<
      LookupTableInstructionInputData['DeactivateLookupTable']
    >([BufferLayout.u32('instruction')]),
  },
  CloseLookupTable: {
    index: 4,
    layout: BufferLayout.struct<
      LookupTableInstructionInputData['CloseLookupTable']
    >([BufferLayout.u32('instruction')]),
  },
});

export class AddressLookupTableInstruction {
  /**
   * @internal
   */
  constructor() {}

  static decodeInstructionType(
    instruction: TransactionInstruction,
  ): LookupTableInstructionType {
    this.checkProgramId(instruction.programId);

    const instructionTypeLayout = BufferLayout.u32('instruction');
    const index = instructionTypeLayout.decode(instruction.data);

    let type: LookupTableInstructionType | undefined;
    for (const [layoutType, layout] of Object.entries(
      LOOKUP_TABLE_INSTRUCTION_LAYOUTS,
    )) {
      if ((layout as any).index == index) {
        type = layoutType as LookupTableInstructionType;
        break;
      }
    }
    if (!type) {
      throw new Error(
        'Invalid Instruction. Should be a LookupTable Instruction',
      );
    }
    return type;
  }

  static decodeCreateLookupTable(
    instruction: TransactionInstruction,
  ): CreateLookupTableParams {
    this.checkProgramId(instruction.programId);
    this.checkKeysLength(instruction.keys, 4);

    const {recentSlot} = decodeData(
      LOOKUP_TABLE_INSTRUCTION_LAYOUTS.CreateLookupTable,
      instruction.data,
    );

    return {
      authority: instruction.keys[1].pubkey,
      payer: instruction.keys[2].pubkey,
      recentSlot: Number(recentSlot),
    };
  }

  static decodeExtendLookupTable(
    instruction: TransactionInstruction,
  ): ExtendLookupTableParams {
    this.checkProgramId(instruction.programId);
    if (instruction.keys.length < 2) {
      throw new Error(
        `invalid instruction; found ${instruction.keys.length} keys, expected at least 2`,
      );
    }

    const {addresses} = decodeData(
      LOOKUP_TABLE_INSTRUCTION_LAYOUTS.ExtendLookupTable,
      instruction.data,
    );
    return {
      lookupTable: instruction.keys[0].pubkey,
      authority: instruction.keys[1].pubkey,
      payer:
        instruction.keys.length > 2 ? instruction.keys[2].pubkey : undefined,
      addresses: addresses.map(buffer => new PublicKey(buffer)),
    };
  }

  static decodeCloseLookupTable(
    instruction: TransactionInstruction,
  ): CloseLookupTableParams {
    this.checkProgramId(instruction.programId);
    this.checkKeysLength(instruction.keys, 3);

    return {
      lookupTable: instruction.keys[0].pubkey,
      authority: instruction.keys[1].pubkey,
      recipient: instruction.keys[2].pubkey,
    };
  }

  static decodeFreezeLookupTable(
    instruction: TransactionInstruction,
  ): FreezeLookupTableParams {
    this.checkProgramId(instruction.programId);
    this.checkKeysLength(instruction.keys, 2);

    return {
      lookupTable: instruction.keys[0].pubkey,
      authority: instruction.keys[1].pubkey,
    };
  }

  static decodeDeactivateLookupTable(
    instruction: TransactionInstruction,
  ): DeactivateLookupTableParams {
    this.checkProgramId(instruction.programId);
    this.checkKeysLength(instruction.keys, 2);

    return {
      lookupTable: instruction.keys[0].pubkey,
      authority: instruction.keys[1].pubkey,
    };
  }

  /**
   * @internal
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(AddressLookupTableProgram.programId)) {
      throw new Error(
        'invalid instruction; programId is not AddressLookupTable Program',
      );
    }
  }
  /**
   * @internal
   */
  static checkKeysLength(keys: Array<any>, expectedLength: number) {
    if (keys.length < expectedLength) {
      throw new Error(
        `invalid instruction; found ${keys.length} keys, expected at least ${expectedLength}`,
      );
    }
  }
}

export class AddressLookupTableProgram {
  /**
   * @internal
   */
  constructor() {}

  static programId: PublicKey = new PublicKey(
    'AddressLookupTab1e1111111111111111111111111',
  );

  static createLookupTable(params: CreateLookupTableParams) {
    const [lookupTableAddress, bumpSeed] = PublicKey.findProgramAddressSync(
      [params.authority.toBuffer(), toBufferLE(BigInt(params.recentSlot), 8)],
      this.programId,
    );

    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.CreateLookupTable;
    const data = encodeData(type, {
      recentSlot: BigInt(params.recentSlot),
      bumpSeed: bumpSeed,
    });

    const keys = [
      {
        pubkey: lookupTableAddress,
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: params.authority,
        isSigner: true,
        isWritable: false,
      },
      {
        pubkey: params.payer,
        isSigner: true,
        isWritable: true,
      },
      {
        pubkey: SystemProgram.programId,
        isSigner: false,
        isWritable: false,
      },
    ];

    return [
      new TransactionInstruction({
        programId: this.programId,
        keys: keys,
        data: data,
      }),
      lookupTableAddress,
    ] as [TransactionInstruction, PublicKey];
  }

  static freezeLookupTable(params: FreezeLookupTableParams) {
    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.FreezeLookupTable;
    const data = encodeData(type);

    const keys = [
      {
        pubkey: params.lookupTable,
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: params.authority,
        isSigner: true,
        isWritable: false,
      },
    ];

    return new TransactionInstruction({
      programId: this.programId,
      keys: keys,
      data: data,
    });
  }

  static extendLookupTable(params: ExtendLookupTableParams) {
    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.ExtendLookupTable;
    const data = encodeData(type, {
      addresses: params.addresses.map(addr => addr.toBytes()),
    });

    const keys = [
      {
        pubkey: params.lookupTable,
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: params.authority,
        isSigner: true,
        isWritable: false,
      },
    ];

    if (params.payer) {
      keys.push(
        {
          pubkey: params.payer,
          isSigner: true,
          isWritable: true,
        },
        {
          pubkey: SystemProgram.programId,
          isSigner: false,
          isWritable: false,
        },
      );
    }

    return new TransactionInstruction({
      programId: this.programId,
      keys: keys,
      data: data,
    });
  }

  static deactivateLookupTable(params: DeactivateLookupTableParams) {
    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.DeactivateLookupTable;
    const data = encodeData(type);

    const keys = [
      {
        pubkey: params.lookupTable,
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: params.authority,
        isSigner: true,
        isWritable: false,
      },
    ];

    return new TransactionInstruction({
      programId: this.programId,
      keys: keys,
      data: data,
    });
  }

  static closeLookupTable(params: CloseLookupTableParams) {
    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.CloseLookupTable;
    const data = encodeData(type);

    const keys = [
      {
        pubkey: params.lookupTable,
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: params.authority,
        isSigner: true,
        isWritable: false,
      },
      {
        pubkey: params.recipient,
        isSigner: false,
        isWritable: true,
      },
    ];

    return new TransactionInstruction({
      programId: this.programId,
      keys: keys,
      data: data,
    });
  }
}
