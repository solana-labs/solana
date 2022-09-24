import bs58 from 'bs58';
import {Buffer} from 'buffer';
import * as BufferLayout from '@solana/buffer-layout';

import {PublicKey, PUBLIC_KEY_LENGTH} from '../publickey';
import type {Blockhash} from '../blockhash';
import * as Layout from '../layout';
import {PACKET_DATA_SIZE, VERSION_PREFIX_MASK} from '../transaction/constants';
import * as shortvec from '../utils/shortvec-encoding';
import {toBuffer} from '../utils/to-buffer';
import {
  MessageHeader,
  MessageAddressTableLookup,
  MessageCompiledInstruction,
} from './index';
import {TransactionInstruction} from '../transaction';
import {CompiledKeys} from './compiled-keys';
import {MessageAccountKeys} from './account-keys';

/**
 * An instruction to execute by a program
 *
 * @property {number} programIdIndex
 * @property {number[]} accounts
 * @property {string} data
 */
export type CompiledInstruction = {
  /** Index into the transaction keys array indicating the program account that executes this instruction */
  programIdIndex: number;
  /** Ordered indices into the transaction keys array indicating which accounts to pass to the program */
  accounts: number[];
  /** The program input data encoded as base 58 */
  data: string;
};

/**
 * Message constructor arguments
 */
export type MessageArgs = {
  /** The message header, identifying signed and read-only `accountKeys` */
  header: MessageHeader;
  /** All the account keys used by this transaction */
  accountKeys: string[] | PublicKey[];
  /** The hash of a recent ledger block */
  recentBlockhash: Blockhash;
  /** Instructions that will be executed in sequence and committed in one atomic transaction if all succeed. */
  instructions: CompiledInstruction[];
};

export type CompileLegacyArgs = {
  payerKey: PublicKey;
  instructions: Array<TransactionInstruction>;
  recentBlockhash: Blockhash;
};

/**
 * List of instructions to be processed atomically
 */
export class Message {
  header: MessageHeader;
  accountKeys: PublicKey[];
  recentBlockhash: Blockhash;
  instructions: CompiledInstruction[];

  private indexToProgramIds: Map<number, PublicKey> = new Map<
    number,
    PublicKey
  >();

  constructor(args: MessageArgs) {
    this.header = args.header;
    this.accountKeys = args.accountKeys.map(account => new PublicKey(account));
    this.recentBlockhash = args.recentBlockhash;
    this.instructions = args.instructions;
    this.instructions.forEach(ix =>
      this.indexToProgramIds.set(
        ix.programIdIndex,
        this.accountKeys[ix.programIdIndex],
      ),
    );
  }

  get version(): 'legacy' {
    return 'legacy';
  }

  get staticAccountKeys(): Array<PublicKey> {
    return this.accountKeys;
  }

  get compiledInstructions(): Array<MessageCompiledInstruction> {
    return this.instructions.map(
      (ix): MessageCompiledInstruction => ({
        programIdIndex: ix.programIdIndex,
        accountKeyIndexes: ix.accounts,
        data: bs58.decode(ix.data),
      }),
    );
  }

  get addressTableLookups(): Array<MessageAddressTableLookup> {
    return [];
  }

  getAccountKeys(): MessageAccountKeys {
    return new MessageAccountKeys(this.staticAccountKeys);
  }

  static compile(args: CompileLegacyArgs): Message {
    const compiledKeys = CompiledKeys.compile(args.instructions, args.payerKey);
    const [header, staticAccountKeys] = compiledKeys.getMessageComponents();
    const accountKeys = new MessageAccountKeys(staticAccountKeys);
    const instructions = accountKeys.compileInstructions(args.instructions).map(
      (ix: MessageCompiledInstruction): CompiledInstruction => ({
        programIdIndex: ix.programIdIndex,
        accounts: ix.accountKeyIndexes,
        data: bs58.encode(ix.data),
      }),
    );
    return new Message({
      header,
      accountKeys: staticAccountKeys,
      recentBlockhash: args.recentBlockhash,
      instructions,
    });
  }

  isAccountSigner(index: number): boolean {
    return index < this.header.numRequiredSignatures;
  }

  isAccountWritable(index: number): boolean {
    const numSignedAccounts = this.header.numRequiredSignatures;
    if (index >= this.header.numRequiredSignatures) {
      const unsignedAccountIndex = index - numSignedAccounts;
      const numUnsignedAccounts = this.accountKeys.length - numSignedAccounts;
      const numWritableUnsignedAccounts =
        numUnsignedAccounts - this.header.numReadonlyUnsignedAccounts;
      return unsignedAccountIndex < numWritableUnsignedAccounts;
    } else {
      const numWritableSignedAccounts =
        numSignedAccounts - this.header.numReadonlySignedAccounts;
      return index < numWritableSignedAccounts;
    }
  }

  isProgramId(index: number): boolean {
    return this.indexToProgramIds.has(index);
  }

  programIds(): PublicKey[] {
    return [...this.indexToProgramIds.values()];
  }

  nonProgramIds(): PublicKey[] {
    return this.accountKeys.filter((_, index) => !this.isProgramId(index));
  }

  serialize(): Buffer {
    const numKeys = this.accountKeys.length;

    let keyCount: number[] = [];
    shortvec.encodeLength(keyCount, numKeys);

    const instructions = this.instructions.map(instruction => {
      const {accounts, programIdIndex} = instruction;
      const data = Array.from(bs58.decode(instruction.data));

      let keyIndicesCount: number[] = [];
      shortvec.encodeLength(keyIndicesCount, accounts.length);

      let dataCount: number[] = [];
      shortvec.encodeLength(dataCount, data.length);

      return {
        programIdIndex,
        keyIndicesCount: Buffer.from(keyIndicesCount),
        keyIndices: accounts,
        dataLength: Buffer.from(dataCount),
        data,
      };
    });

    let instructionCount: number[] = [];
    shortvec.encodeLength(instructionCount, instructions.length);
    let instructionBuffer = Buffer.alloc(PACKET_DATA_SIZE);
    Buffer.from(instructionCount).copy(instructionBuffer);
    let instructionBufferLength = instructionCount.length;

    instructions.forEach(instruction => {
      const instructionLayout = BufferLayout.struct<
        Readonly<{
          data: number[];
          dataLength: Uint8Array;
          keyIndices: number[];
          keyIndicesCount: Uint8Array;
          programIdIndex: number;
        }>
      >([
        BufferLayout.u8('programIdIndex'),

        BufferLayout.blob(
          instruction.keyIndicesCount.length,
          'keyIndicesCount',
        ),
        BufferLayout.seq(
          BufferLayout.u8('keyIndex'),
          instruction.keyIndices.length,
          'keyIndices',
        ),
        BufferLayout.blob(instruction.dataLength.length, 'dataLength'),
        BufferLayout.seq(
          BufferLayout.u8('userdatum'),
          instruction.data.length,
          'data',
        ),
      ]);
      const length = instructionLayout.encode(
        instruction,
        instructionBuffer,
        instructionBufferLength,
      );
      instructionBufferLength += length;
    });
    instructionBuffer = instructionBuffer.slice(0, instructionBufferLength);

    const signDataLayout = BufferLayout.struct<
      Readonly<{
        keyCount: Uint8Array;
        keys: Uint8Array[];
        numReadonlySignedAccounts: Uint8Array;
        numReadonlyUnsignedAccounts: Uint8Array;
        numRequiredSignatures: Uint8Array;
        recentBlockhash: Uint8Array;
      }>
    >([
      BufferLayout.blob(1, 'numRequiredSignatures'),
      BufferLayout.blob(1, 'numReadonlySignedAccounts'),
      BufferLayout.blob(1, 'numReadonlyUnsignedAccounts'),
      BufferLayout.blob(keyCount.length, 'keyCount'),
      BufferLayout.seq(Layout.publicKey('key'), numKeys, 'keys'),
      Layout.publicKey('recentBlockhash'),
    ]);

    const transaction = {
      numRequiredSignatures: Buffer.from([this.header.numRequiredSignatures]),
      numReadonlySignedAccounts: Buffer.from([
        this.header.numReadonlySignedAccounts,
      ]),
      numReadonlyUnsignedAccounts: Buffer.from([
        this.header.numReadonlyUnsignedAccounts,
      ]),
      keyCount: Buffer.from(keyCount),
      keys: this.accountKeys.map(key => toBuffer(key.toBytes())),
      recentBlockhash: bs58.decode(this.recentBlockhash),
    };

    let signData = Buffer.alloc(2048);
    const length = signDataLayout.encode(transaction, signData);
    instructionBuffer.copy(signData, length);
    return signData.slice(0, length + instructionBuffer.length);
  }

  /**
   * Decode a compiled message into a Message object.
   */
  static from(buffer: Buffer | Uint8Array | Array<number>): Message {
    // Slice up wire data
    let byteArray = [...buffer];

    const numRequiredSignatures = byteArray.shift() as number;
    if (
      numRequiredSignatures !==
      (numRequiredSignatures & VERSION_PREFIX_MASK)
    ) {
      throw new Error(
        'Versioned messages must be deserialized with VersionedMessage.deserialize()',
      );
    }

    const numReadonlySignedAccounts = byteArray.shift() as number;
    const numReadonlyUnsignedAccounts = byteArray.shift() as number;

    const accountCount = shortvec.decodeLength(byteArray);
    let accountKeys = [];
    for (let i = 0; i < accountCount; i++) {
      const account = byteArray.slice(0, PUBLIC_KEY_LENGTH);
      byteArray = byteArray.slice(PUBLIC_KEY_LENGTH);
      accountKeys.push(new PublicKey(Buffer.from(account)));
    }

    const recentBlockhash = byteArray.slice(0, PUBLIC_KEY_LENGTH);
    byteArray = byteArray.slice(PUBLIC_KEY_LENGTH);

    const instructionCount = shortvec.decodeLength(byteArray);
    let instructions: CompiledInstruction[] = [];
    for (let i = 0; i < instructionCount; i++) {
      const programIdIndex = byteArray.shift() as number;
      const accountCount = shortvec.decodeLength(byteArray);
      const accounts = byteArray.slice(0, accountCount);
      byteArray = byteArray.slice(accountCount);
      const dataLength = shortvec.decodeLength(byteArray);
      const dataSlice = byteArray.slice(0, dataLength);
      const data = bs58.encode(Buffer.from(dataSlice));
      byteArray = byteArray.slice(dataLength);
      instructions.push({
        programIdIndex,
        accounts,
        data,
      });
    }

    const messageArgs = {
      header: {
        numRequiredSignatures,
        numReadonlySignedAccounts,
        numReadonlyUnsignedAccounts,
      },
      recentBlockhash: bs58.encode(Buffer.from(recentBlockhash)),
      accountKeys,
      instructions,
    };

    return new Message(messageArgs);
  }
}
