import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from './layout';
import { PublicKey } from './publickey';
import { SystemProgram } from './system-program';
import { TransactionInstruction } from './transaction';
import { encodeData, IInstructionInputData } from './instruction';

export type CreateLookupTableParams = {
    lookupTable: PublicKey;
    authority: PublicKey;
    payer: PublicKey;
    slot: number;
    bumpSeed: number;
};

export type FreezeLookupTableParams = {
    lookupTable: PublicKey;
    authority: PublicKey;
};

export type ExtendLookupTableParams = {
    lookupTable: PublicKey;
    authority: PublicKey;
    payer: PublicKey;
    addresses: Array<PublicKey>;
};

export type DeactivateLookupTableParams = {
    lookupTable: PublicKey;
    authority: PublicKey;
};

export type CloseLookupTableParams = {
    lookupTable: PublicKey;
    authority: PublicKey;
    recipient: PublicKey;
};

export type LookupTableInstructionType =
    | 'Create'
    | 'Extend'
    | 'Close'
    | 'Freeze'
    | 'Deactivate';

export type LookupTableInstructionInputData = {
    Create: IInstructionInputData &
        Readonly<{
            slot: number;
            bumpSeed: number;
        }>;
    Freeze: IInstructionInputData;
    Extend: IInstructionInputData &
        Readonly<{
            numberOfAddresses: number;
            addresses: Array<Uint8Array>;
        }>;
    Deactivate: IInstructionInputData;
    Close: IInstructionInputData;
};

export const LOOKUP_TABLE_INSTRUCTION_LAYOUTS = Object.freeze({
    Create: {
        index: 0,
        layout: BufferLayout.struct<LookupTableInstructionInputData['Create']>([
            BufferLayout.u32('instruction'),
            BufferLayout.nu64('slot'),
            BufferLayout.u8('bumpSeed'),
        ]),
    },
    Freeze: {
        index: 1,
        layout: BufferLayout.struct<LookupTableInstructionInputData['Freeze']>([
            BufferLayout.u32('instruction'),
        ]),
    },
    Extend: (numberOfAddresses: number) => {
        return {
            index: 2,
            layout: BufferLayout.struct<
                LookupTableInstructionInputData['Extend']
            >([
                BufferLayout.u32('instruction'),
                BufferLayout.nu64('numberOfAddresses'),
                BufferLayout.seq(
                    Layout.publicKey(),
                    numberOfAddresses,
                    'addresses'
                ),
            ]),
        };
    },
    Deactivate: {
        index: 3,
        layout: BufferLayout.struct<
            LookupTableInstructionInputData['Deactivate']
        >([BufferLayout.u32('instruction')]),
    },
    Close: {
        index: 4,
        layout: BufferLayout.struct<LookupTableInstructionInputData['Close']>([
            BufferLayout.u32('instruction'),
        ]),
    },
});

export class AddressLookupTableProgram {
    /**
     * @internal
     */
    constructor() {}

    static programId: PublicKey = new PublicKey(
        'AddressLookupTab1e1111111111111111111111111'
    );

    static async createLookupTable(params: CreateLookupTableParams) {
        const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.Create;
        const data = encodeData(type, {
            slot: params.slot,
            bumpSeed: params.bumpSeed,
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

        return new TransactionInstruction({
            programId: this.programId,
            keys: keys,
            data: data,
        });
    }

    static freezeLookupTable(params: FreezeLookupTableParams) {
        const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.Freeze;
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
        const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.Extend(
            params.addresses.length
        );
        const data = encodeData(type, {
            numberOfAddresses: params.addresses.length,
            addresses: params.addresses.map((addr) => addr.toBuffer()),
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

        return new TransactionInstruction({
            programId: this.programId,
            keys: keys,
            data: data,
        });
    }

    static deactivateLookupTable(params: DeactivateLookupTableParams) {
        const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.Deactivate;
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
        const type = LOOKUP_TABLE_INSTRUCTION_LAYOUTS.Close;
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
                isWritable: false,
            },
        ];

        return new TransactionInstruction({
            programId: this.programId,
            keys: keys,
            data: data,
        });
    }
}
