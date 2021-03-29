import bs58 from 'bs58';
import {Buffer} from 'buffer';
import * as BufferLayout from 'buffer-layout';

import {PublicKey} from './publickey';
import type {Blockhash} from './blockhash';
import * as Layout from './layout';
import {PACKET_DATA_SIZE} from './transaction';
import * as shortvec from './util/shortvec-encoding';
import {toBuffer} from './util/to-buffer';

/**
 * The message header, identifying signed and read-only account
 *
 * @typedef {Object} MessageHeader
 * @property {number} numRequiredSignatures The number of signatures required for this message to be considered valid. The
 * signatures must match the first `numRequiredSignatures` of `accountKeys`.
 * @property {number} numReadonlySignedAccounts: The last `numReadonlySignedAccounts` of the signed keys are read-only accounts
 * @property {number} numReadonlyUnsignedAccounts The last `numReadonlySignedAccounts` of the unsigned keys are read-only accounts
 */
export type MessageHeader = {
  numRequiredSignatures: number;
  numReadonlySignedAccounts: number;
  numReadonlyUnsignedAccounts: number;
};

/**
 * An instruction to execute by a program
 *
 * @typedef {Object} CompiledInstruction
 * @property {number} programIdIndex Index into the transaction keys array indicating the program account that executes this instruction
 * @property {number[]} accounts Ordered indices into the transaction keys array indicating which accounts to pass to the program
 * @property {string} data The program input data encoded as base 58
 */
export type CompiledInstruction = {
  programIdIndex: number;
  accounts: number[];
  data: string;
};

/**
 * Message constructor arguments
 *
 * @typedef {Object} MessageArgs
 * @property {MessageHeader} header The message header, identifying signed and read-only `accountKeys`
 * @property {string[]} accounts All the account keys used by this transaction
 * @property {Blockhash} recentBlockhash The hash of a recent ledger block
 * @property {CompiledInstruction[]} instructions Instructions that will be executed in sequence and committed in one atomic transaction if all succeed.
 */
export type MessageArgs = {
  header: MessageHeader;
  accountKeys: string[];
  recentBlockhash: Blockhash;
  instructions: CompiledInstruction[];
};

const PUBKEY_LENGTH = 32;

/**
 * List of instructions to be processed atomically
 */
export class Message {
  header: MessageHeader;
  accountKeys: PublicKey[];
  recentBlockhash: Blockhash;
  instructions: CompiledInstruction[];

  constructor(args: MessageArgs) {
    this.header = args.header;
    this.accountKeys = args.accountKeys.map(account => new PublicKey(account));
    this.recentBlockhash = args.recentBlockhash;
    this.instructions = args.instructions;
  }

  isAccountWritable(index: number): boolean {
    return (
      index <
        this.header.numRequiredSignatures -
          this.header.numReadonlySignedAccounts ||
      (index >= this.header.numRequiredSignatures &&
        index <
          this.accountKeys.length - this.header.numReadonlyUnsignedAccounts)
    );
  }

  serialize(): Buffer {
    const numKeys = this.accountKeys.length;

    let keyCount: number[] = [];
    shortvec.encodeLength(keyCount, numKeys);

    const instructions = this.instructions.map(instruction => {
      const {accounts, programIdIndex} = instruction;
      const data = bs58.decode(instruction.data);

      let keyIndicesCount: number[] = [];
      shortvec.encodeLength(keyIndicesCount, accounts.length);

      let dataCount: number[] = [];
      shortvec.encodeLength(dataCount, data.length);

      return {
        programIdIndex,
        keyIndicesCount: Buffer.from(keyIndicesCount),
        keyIndices: Buffer.from(accounts),
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
      const instructionLayout = BufferLayout.struct([
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

    const signDataLayout = BufferLayout.struct([
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
    const numReadonlySignedAccounts = byteArray.shift() as number;
    const numReadonlyUnsignedAccounts = byteArray.shift() as number;

    const accountCount = shortvec.decodeLength(byteArray);
    let accountKeys = [];
    for (let i = 0; i < accountCount; i++) {
      const account = byteArray.slice(0, PUBKEY_LENGTH);
      byteArray = byteArray.slice(PUBKEY_LENGTH);
      accountKeys.push(bs58.encode(Buffer.from(account)));
    }

    const recentBlockhash = byteArray.slice(0, PUBKEY_LENGTH);
    byteArray = byteArray.slice(PUBKEY_LENGTH);

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
