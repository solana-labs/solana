// @flow

import bs58 from 'bs58';
import * as BufferLayout from 'buffer-layout';

import {PublicKey} from './publickey';
import type {Blockhash} from './blockhash';
import * as Layout from './layout';
import {PACKET_DATA_SIZE} from './transaction';
import * as shortvec from './util/shortvec-encoding';

/**
 * The message header, identifying signed and read-only account
 *
 * @typedef {Object} MessageHeader
 * @property {number} numRequiredSignatures The number of signatures required for this message to be considered valid
 * @property {number} numReadonlySignedAccounts: The last `numReadonlySignedAccounts` of the signed keys are read-only accounts
 * @property {number} numReadonlyUnsignedAccounts The last `numReadonlySignedAccounts` of the unsigned keys are read-only accounts
 */
export type MessageHeader = {
  numRequiredSignatures: number,
  numReadonlySignedAccounts: number,
  numReadonlyUnsignedAccounts: number,
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
  programIdIndex: number,
  accounts: number[],
  data: string,
};

/**
 * Message constructor arguments
 *
 * @typedef {Object} MessageArgs
 * @property {MessageHeader} header The message header, identifying signed and read-only `accountKeys`
 * @property {PublicKey[]} accounts All the account keys used by this transaction
 * @property {Blockhash} recentBlockhash The hash of a recent ledger block
 * @property {CompiledInstruction[]} instructions Instructions that will be executed in sequence and committed in one atomic transaction if all succeed.
 */
type MessageArgs = {
  header: MessageHeader,
  accountKeys: PublicKey[],
  recentBlockhash: Blockhash,
  instructions: CompiledInstruction[],
};

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
    this.accountKeys = args.accountKeys;
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

    let keyCount = [];
    shortvec.encodeLength(keyCount, numKeys);

    const instructions = this.instructions.map(instruction => {
      const {accounts, programIdIndex} = instruction;
      const data = bs58.decode(instruction.data);

      let keyIndicesCount = [];
      shortvec.encodeLength(keyIndicesCount, accounts.length);

      let dataCount = [];
      shortvec.encodeLength(dataCount, data.length);

      return {
        programIdIndex,
        keyIndicesCount: Buffer.from(keyIndicesCount),
        keyIndices: Buffer.from(accounts),
        dataLength: Buffer.from(dataCount),
        data,
      };
    });

    let instructionCount = [];
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
      keys: this.accountKeys.map(key => key.toBuffer()),
      recentBlockhash: bs58.decode(this.recentBlockhash),
    };

    let signData = Buffer.alloc(2048);
    const length = signDataLayout.encode(transaction, signData);
    instructionBuffer.copy(signData, length);
    return signData.slice(0, length + instructionBuffer.length);
  }
}
