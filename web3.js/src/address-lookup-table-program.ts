import {toBufferLE} from 'bigint-buffer';
import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from './layout';
import {PublicKey} from './publickey';
import * as bigintLayout from './util/bigint';
import {SystemProgram} from './system-program';
import {TransactionInstruction} from './transaction';
import {encodeData, IInstructionInputData} from './instruction';

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

export type LookupTableInstructionType =
  | 'CreateLookupTable'
  | 'ExtendLookupTable'
  | 'CloseLookupTable'
  | 'FreezeLookupTable'
  | 'DeactivateLookupTable';

export type LookupTableInstructionInputData = {
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
  ExtendLookupTable: (numberOfAddresses: number) => {
    return {
      index: 2,
      layout: BufferLayout.struct<
        LookupTableInstructionInputData['ExtendLookupTable']
      >([
        BufferLayout.u32('instruction'),
        bigintLayout.u64('numberOfAddresses'),
        BufferLayout.seq(Layout.publicKey(), numberOfAddresses, 'addresses'),
      ]),
    };
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
      recentSlot: params.recentSlot,
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
    const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.ExtendLookupTable(
      params.addresses.length,
    );
    const data = encodeData(type, {
      numberOfAddresses: BigInt(params.addresses.length),
      addresses: params.addresses.map(addr => addr.toBuffer()),
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
