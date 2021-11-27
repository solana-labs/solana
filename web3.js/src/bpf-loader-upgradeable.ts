import * as BufferLayout from '@solana/buffer-layout';

import {encodeData, InstructionType} from './instruction';
import * as Layout from './layout';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {SYSVAR_CLOCK_PUBKEY, SYSVAR_RENT_PUBKEY} from './sysvar';
import {
  Transaction,
  TransactionInstruction,
  PACKET_DATA_SIZE,
} from './transaction';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from './connection';
import type {Signer} from './keypair';

export const BPF_LOADER_UPGRADEABLE_PROGRAM_ID = new PublicKey(
  'BPFLoaderUpgradeab1e11111111111111111111111',
);

/**
 * An enumeration of valid BpfUpgradeableLoaderInstructionType's
 */
export type BpfUpgradeableLoaderInstructionType =
  | 'InitializeBuffer'
  | 'Write'
  | 'DeployWithMaxDataLen'
  | 'Upgrade'
  | 'SetAuthority'
  | 'Close';

/**
 * An enumeration of valid system InstructionType's
 * @internal
 */
export const BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS: {
  [type in BpfUpgradeableLoaderInstructionType]: InstructionType;
} = Object.freeze({
  InitializeBuffer: {
    index: 0,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  Write: {
    index: 1,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.u32('offset'),
      Layout.rustVecBytes('bytes'),
    ]),
  },
  DeployWithMaxDataLen: {
    index: 2,
    layout: BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.u32('maxDataLen'),
      BufferLayout.u32('maxDataLenPadding'),
    ]),
  },
  Upgrade: {
    index: 3,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  SetAuthority: {
    index: 4,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
  Close: {
    index: 5,
    layout: BufferLayout.struct([BufferLayout.u32('instruction')]),
  },
});

/**
 * Initialize buffer transaction params
 */
export type InitializeBufferParams = {
  /** Public key of the buffer account */
  bufferPubkey: PublicKey;
  /** Public key to set as authority of the initialized buffer */
  authorityPubkey: PublicKey;
};

/**
 * Write transaction params
 */
export type WriteParams = {
  /** Offset at which to write the given bytes. */
  offset: number;
  /** Chunk of program data */
  bytes: Buffer;
  /** Public key of the buffer account */
  bufferPubkey: PublicKey;
  /** Public key to set as authority of the initialized buffer */
  authorityPubkey: PublicKey;
};

/**
 * Deploy a program transaction params
 */
export type DeployWithMaxProgramLenParams = {
  /** Maximum length that the program can be upgraded to. */
  maxDataLen: number;
  /** The uninitialized Program account */
  programPubkey: PublicKey;
  /** The buffer account where the program data has been written. The buffer account’s authority must match the program’s authority */
  bufferPubkey: PublicKey;
  /** The program’s authority */
  upgradeAuthorityPubkey: PublicKey;
  /** The payer account that will pay to create the ProgramData account */
  payerPubkey: PublicKey;
};

/**
 * Upgrade transaction params
 */
export type UpgradeParams = {
  /** The program account */
  programPubkey: PublicKey;
  /** The buffer account where the program data has been written. The buffer account’s authority must match the program’s authority */
  bufferPubkey: PublicKey;
  /** The spill account */
  spillPubkey: PublicKey;
  /** The program’s authority */
  authorityPubkey: PublicKey;
};

/**
 * Update buffer authority transaction params
 */
export type SetBufferAuthorityParams = {
  /** The buffer account where the program data has been written */
  bufferPubkey: PublicKey;
  /** The buffer's authority */
  authorityPubkey: PublicKey;
  /** New buffer's authority */
  newAuthorityPubkey: PublicKey;
};

/**
 * Update program authority transaction params
 */
export type SetUpgradeAuthorityParams = {
  /** The program account */
  programPubkey: PublicKey;
  /** The current authority */
  authorityPubkey: PublicKey;
  /** The new authority, optional, if omitted then the program will not be upgradeable */
  newAuthorityPubkey: PublicKey | undefined;
};

/**
 * Close account transaction params
 */
export type CloseParams = {
  /** The account to close */
  closePubkey: PublicKey;
  /** The account to deposit the closed account’s lamports */
  recipientPubkey: PublicKey;
  /** The account’s authority, Optional, required for initialized accounts */
  authorityPubkey: PublicKey | undefined;
  /** The associated Program account if the account to close is a ProgramData account */
  programPubkey: PublicKey | undefined;
};

/**
 * Factory class for transactions to interact with the BpfLoaderUpgradeable program
 */
export class BpfLoaderUpgradeableProgram {
  /**
   * @internal
   */
  constructor() {}

  /**
   * Public key that identifies the BpfLoaderUpgradeable program
   */
  static programId: PublicKey = BPF_LOADER_UPGRADEABLE_PROGRAM_ID;

  /**
   * Derive programData address from program
   */
  static async getProgramDataAddress(program: PublicKey): Promise<PublicKey> {
    return (
      await PublicKey.findProgramAddress([program.toBuffer()], this.programId)
    )[0];
  }

  /**
   * Generate a transaction instruction that initialize buffer account
   */
  static initializeBuffer(
    params: InitializeBufferParams,
  ): TransactionInstruction {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.InitializeBuffer;
    const data = encodeData(type, {});

    return new TransactionInstruction({
      keys: [
        {pubkey: params.bufferPubkey, isSigner: false, isWritable: true},
        {pubkey: params.authorityPubkey, isSigner: false, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that write a chunk of program data
   *   to a buffer account
   */
  static write(params: WriteParams): TransactionInstruction {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.Write;
    const data = encodeData(type, {
      offset: params.offset,
      bytes: params.bytes,
    });

    return new TransactionInstruction({
      keys: [
        {pubkey: params.bufferPubkey, isSigner: false, isWritable: true},
        {pubkey: params.authorityPubkey, isSigner: true, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that deploy a program with a specified
   *   maximum program length
   */
  static async deployWithMaxProgramLen(
    params: DeployWithMaxProgramLenParams,
  ): Promise<TransactionInstruction> {
    const type =
      BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.DeployWithMaxDataLen;
    const data = encodeData(type, {
      maxDataLen: params.maxDataLen,
    });

    const programDataPubkey = await this.getProgramDataAddress(
      params.programPubkey,
    );

    return new TransactionInstruction({
      keys: [
        {pubkey: params.payerPubkey, isSigner: true, isWritable: true},
        {pubkey: programDataPubkey, isSigner: false, isWritable: true},
        {pubkey: params.programPubkey, isSigner: false, isWritable: true},
        {pubkey: params.bufferPubkey, isSigner: false, isWritable: true},
        {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: SystemProgram.programId, isSigner: false, isWritable: false},
        {
          pubkey: params.upgradeAuthorityPubkey,
          isSigner: true,
          isWritable: false,
        },
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that upgrade a program
   */
  static async upgrade(params: UpgradeParams): Promise<TransactionInstruction> {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.Upgrade;
    const data = encodeData(type, {});

    const programDataPubkey = await this.getProgramDataAddress(
      params.programPubkey,
    );

    return new TransactionInstruction({
      keys: [
        {pubkey: programDataPubkey, isSigner: false, isWritable: true},
        {pubkey: params.programPubkey, isSigner: false, isWritable: true},
        {pubkey: params.bufferPubkey, isSigner: false, isWritable: true},
        {pubkey: params.spillPubkey, isSigner: true, isWritable: true},
        {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
        {pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false},
        {
          pubkey: params.authorityPubkey,
          isSigner: true,
          isWritable: false,
        },
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that set a new buffer authority
   */
  static setBufferAuthority(
    params: SetBufferAuthorityParams,
  ): TransactionInstruction {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.SetAuthority;
    const data = encodeData(type, {});

    return new TransactionInstruction({
      keys: [
        {pubkey: params.bufferPubkey, isSigner: false, isWritable: true},
        {
          pubkey: params.authorityPubkey,
          isSigner: true,
          isWritable: false,
        },
        {pubkey: params.newAuthorityPubkey, isSigner: false, isWritable: false},
      ],
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that set a new program authority
   */
  static async setUpgradeAuthority(
    params: SetUpgradeAuthorityParams,
  ): Promise<TransactionInstruction> {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.SetAuthority;
    const data = encodeData(type, {});

    const programDataPubkey = await this.getProgramDataAddress(
      params.programPubkey,
    );

    const keys = [
      {pubkey: programDataPubkey, isSigner: false, isWritable: true},
      {
        pubkey: params.authorityPubkey,
        isSigner: true,
        isWritable: false,
      },
    ];

    if (params.newAuthorityPubkey) {
      keys.push({
        pubkey: params.newAuthorityPubkey,
        isSigner: false,
        isWritable: false,
      });
    }

    return new TransactionInstruction({
      keys,
      programId: this.programId,
      data,
    });
  }

  /**
   * Generate a transaction instruction that close program, buffer, or
   *   uninitialized account
   */
  static close(params: CloseParams): TransactionInstruction {
    const type = BPF_UPGRADEABLE_LOADER_INSTRUCTION_LAYOUTS.Close;
    const data = encodeData(type, {});

    const keys = [
      {pubkey: params.closePubkey, isSigner: false, isWritable: true},
      {
        pubkey: params.recipientPubkey,
        isSigner: false,
        isWritable: true,
      },
    ];

    if (params.authorityPubkey) {
      keys.push({
        pubkey: params.authorityPubkey,
        isSigner: true,
        isWritable: false,
      });
    }

    if (params.programPubkey) {
      keys.push({
        pubkey: params.programPubkey,
        isSigner: false,
        isWritable: true,
      });
    }

    return new TransactionInstruction({
      keys,
      programId: this.programId,
      data,
    });
  }
}

/**
 * BpfLoaderUpgradeable program interface
 */
export class BpfLoaderUpgradeable {
  /**
   * @internal
   */
  constructor() {}

  /**
   * Buffer account size without data
   */
  static BUFFER_HEADER_SIZE: number = 37; // Option<Pubkey>

  /**
   * Program account size
   */
  static BUFFER_PROGRAM_SIZE: number = 36; // Pubkey

  /**
   * ProgramData account size without data
   */
  static BUFFER_PROGRAM_DATA_HEADER_SIZE: number = 45; // usize + Option<Pubkey>

  /**
   * Maximal chunk of the data per transaction
   */
  static WRITE_CHUNK_SIZE: number = PACKET_DATA_SIZE - 316; // Data with 2 signatures

  /**
   * Get buffer account size
   */
  static getBufferAccountSize(programLen: number): number {
    return this.BUFFER_HEADER_SIZE + programLen;
  }

  /**
   * Create and initialize buffer account
   */
  static async createBuffer(
    connection: Connection,
    payer: Signer,
    buffer: Signer,
    authority: PublicKey,
    lamports: number,
    programLen: number,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: buffer.publicKey,
        lamports,
        space: this.getBufferAccountSize(programLen),
        programId: BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
      }),
    );
    transaction.add(
      BpfLoaderUpgradeableProgram.initializeBuffer({
        bufferPubkey: buffer.publicKey,
        authorityPubkey: authority,
      }),
    );
    await sendAndConfirmTransaction(connection, transaction, [payer, buffer], {
      commitment: 'confirmed',
    });
  }

  /**
   * Update buffer authority
   */
  static async setBufferAuthority(
    connection: Connection,
    payer: Signer,
    buffer: PublicKey,
    authority: Signer,
    newAuthority: PublicKey,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      BpfLoaderUpgradeableProgram.setBufferAuthority({
        bufferPubkey: buffer,
        authorityPubkey: authority.publicKey,
        newAuthorityPubkey: newAuthority,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, authority],
      {
        commitment: 'confirmed',
      },
    );
  }

  /**
   * Load programData to initialized buffer account
   */
  static async loadBuffer(
    connection: Connection,
    payer: Signer,
    buffer: PublicKey,
    authority: Signer,
    programData: Buffer,
    loadConcurrency: number = 10,
  ) {
    let bytesOffset = 0;
    await Promise.all(
      new Array(loadConcurrency).fill(null).map(async () => {
        for (;;) {
          const offset = bytesOffset;
          bytesOffset += BpfLoaderUpgradeable.WRITE_CHUNK_SIZE;

          const bytes = programData.slice(
            offset,
            offset + BpfLoaderUpgradeable.WRITE_CHUNK_SIZE,
          );
          if (bytes.length === 0) {
            break;
          }

          const transaction: Transaction = new Transaction();
          transaction.add(
            BpfLoaderUpgradeableProgram.write({
              offset,
              bytes,
              bufferPubkey: buffer,
              authorityPubkey: authority.publicKey,
            }),
          );
          await sendAndConfirmTransaction(
            connection,
            transaction,
            [payer, authority],
            {
              commitment: 'confirmed',
            },
          );
        }
      }),
    );
  }

  /**
   * Close buffer account and withdraw funds
   */
  static async closeBuffer(
    connection: Connection,
    payer: Signer,
    buffer: PublicKey,
    authority: Signer,
    recipient: PublicKey,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      BpfLoaderUpgradeableProgram.close({
        closePubkey: buffer,
        recipientPubkey: recipient,
        authorityPubkey: authority.publicKey,
        programPubkey: undefined,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, authority],
      {
        commitment: 'confirmed',
      },
    );
  }

  /**
   * create program account from initialized buffer
   */
  static async deployProgram(
    connection: Connection,
    payer: Signer,
    buffer: PublicKey,
    bufferAuthority: Signer,
    program: Signer,
    programLamports: number,
    maxDataLen: number,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: program.publicKey,
        lamports: programLamports,
        space: this.BUFFER_PROGRAM_SIZE,
        programId: BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
      }),
    );
    transaction.add(
      await BpfLoaderUpgradeableProgram.deployWithMaxProgramLen({
        maxDataLen,
        programPubkey: program.publicKey,
        bufferPubkey: buffer,
        upgradeAuthorityPubkey: bufferAuthority.publicKey,
        payerPubkey: payer.publicKey,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, program, bufferAuthority],
      {
        commitment: 'confirmed',
      },
    );
  }

  /**
   * Update program authority
   */
  static async setProgramAuthority(
    connection: Connection,
    payer: Signer,
    program: PublicKey,
    authority: Signer,
    newAuthority: PublicKey | undefined,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      await BpfLoaderUpgradeableProgram.setUpgradeAuthority({
        programPubkey: program,
        authorityPubkey: authority.publicKey,
        newAuthorityPubkey: newAuthority,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, authority],
      {
        commitment: 'confirmed',
      },
    );
  }

  /**
   * Upgrade a program
   */
  static async upgradeProgram(
    connection: Connection,
    payer: Signer,
    program: PublicKey,
    authority: Signer,
    buffer: PublicKey,
    spill: PublicKey,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      await BpfLoaderUpgradeableProgram.upgrade({
        programPubkey: program,
        bufferPubkey: buffer,
        spillPubkey: spill,
        authorityPubkey: authority.publicKey,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, authority],
      {
        commitment: 'confirmed',
      },
    );
  }

  /**
   * Close program account and withdraw funds
   */
  static async closeProgram(
    connection: Connection,
    payer: Signer,
    program: PublicKey,
    authority: Signer,
    recipient: PublicKey,
  ) {
    const transaction: Transaction = new Transaction();
    transaction.add(
      BpfLoaderUpgradeableProgram.close({
        closePubkey: await BpfLoaderUpgradeableProgram.getProgramDataAddress(
          program,
        ),
        recipientPubkey: recipient,
        authorityPubkey: authority.publicKey,
        programPubkey: program,
      }),
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer, authority],
      {
        commitment: 'confirmed',
      },
    );
  }
}
