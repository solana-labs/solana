// @flow

import invariant from 'assert';
import * as BufferLayout from 'buffer-layout';
import nacl from 'tweetnacl';
import bs58 from 'bs58';

import * as Layout from './layout';
import {PublicKey} from './publickey';
import {Account} from './account';
import * as shortvec from './util/shortvec-encoding';
import type {Blockhash} from './blockhash';

/**
 * @typedef {string} TransactionSignature
 */
export type TransactionSignature = string;

/**
 * Default (empty) signature
 *
 * Signatures are 64 bytes in length
 */
const DEFAULT_SIGNATURE = Buffer.alloc(64).fill(0);

/**
 * Maximum over-the-wire size of a Transaction
 *
 * 1280 is IPv6 minimum MTU
 * 40 bytes is the size of the IPv6 header
 * 8 bytes is the size of the fragment header
 */
export const PACKET_DATA_SIZE = 1280 - 40 - 8;

const PUBKEY_LENGTH = 32;
const SIGNATURE_LENGTH = 64;

/**
 * List of TransactionInstruction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionInstructionCtorFields
 * @property {?Array<PublicKey>} keys
 * @property {?PublicKey} programId
 * @property {?Buffer} data
 */
export type TransactionInstructionCtorFields = {|
  keys?: Array<{pubkey: PublicKey, isSigner: boolean, isWritable: boolean}>,
  programId?: PublicKey,
  data?: Buffer,
|};

/**
 * Transaction Instruction class
 */
export class TransactionInstruction {
  /**
   * Public keys to include in this transaction
   * Boolean represents whether this pubkey needs to sign the transaction
   */
  keys: Array<{
    pubkey: PublicKey,
    isSigner: boolean,
    isWritable: boolean,
  }> = [];

  /**
   * Program Id to execute
   */
  programId: PublicKey;

  /**
   * Program input
   */
  data: Buffer = Buffer.alloc(0);

  constructor(opts?: TransactionInstructionCtorFields) {
    opts && Object.assign(this, opts);
  }
}

/**
 * @private
 */
type SignaturePubkeyPair = {|
  signature: Buffer | null,
  publicKey: PublicKey,
|};

/**
 * List of Transaction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionCtorFields
 * @property (?recentBlockhash} A recent block hash
 * @property (?signatures} One or more signatures
 *
 */
type TransactionCtorFields = {|
  recentBlockhash?: Blockhash | null,
  nonceInfo?: NonceInformation | null,
  signatures?: Array<SignaturePubkeyPair>,
|};

/**
 * NonceInformation to be used to build a Transaction.
 *
 * @typedef {Object} NonceInformation
 * @property {nonce} The current Nonce blockhash
 * @property {nonceInstruction} The AdvanceNonceAccount Instruction
 */
type NonceInformation = {|
  nonce: Blockhash,
  nonceInstruction: TransactionInstruction,
|};

/**
 * Transaction class
 */
export class Transaction {
  /**
   * Signatures for the transaction.  Typically created by invoking the
   * `sign()` method
   */
  signatures: Array<SignaturePubkeyPair> = [];

  /**
   * The first (payer) Transaction signature
   */
  get signature(): Buffer | null {
    if (this.signatures.length > 0) {
      return this.signatures[0].signature;
    }
    return null;
  }

  /**
   * The instructions to atomically execute
   */
  instructions: Array<TransactionInstruction> = [];

  /**
   * A recent transaction id.  Must be populated by the caller
   */
  recentBlockhash: Blockhash | null;

  /**
   * Optional Nonce information. If populated, transaction will use a durable
   * Nonce hash instead of a recentBlockhash. Must be populated by the caller
   */
  nonceInfo: NonceInformation | null;

  /**
   * Construct an empty Transaction
   */
  constructor(opts?: TransactionCtorFields) {
    opts && Object.assign(this, opts);
  }

  /**
   * Add one or more instructions to this Transaction
   */
  add(
    ...items: Array<
      Transaction | TransactionInstruction | TransactionInstructionCtorFields,
    >
  ): Transaction {
    if (items.length === 0) {
      throw new Error('No instructions');
    }

    items.forEach(item => {
      if (item instanceof Transaction) {
        this.instructions = this.instructions.concat(item.instructions);
      } else if (item instanceof TransactionInstruction) {
        this.instructions.push(item);
      } else {
        this.instructions.push(new TransactionInstruction(item));
      }
    });
    return this;
  }

  /**
   * Get a buffer of the Transaction data that need to be covered by signatures
   */
  get signData(): Buffer {
    const {nonceInfo} = this;
    if (nonceInfo && this.instructions[0] != nonceInfo.nonceInstruction) {
      this.recentBlockhash = nonceInfo.nonce;
      this.instructions.unshift(nonceInfo.nonceInstruction);
    }
    const {recentBlockhash} = this;
    if (!recentBlockhash) {
      throw new Error('Transaction recentBlockhash required');
    }

    if (this.instructions.length < 1) {
      throw new Error('No instructions provided');
    }

    const keys = this.signatures.map(({publicKey}) => publicKey.toString());
    let numReadonlySignedAccounts = 0;
    let numReadonlyUnsignedAccounts = 0;

    const programIds = [];

    const allKeys = [];
    this.instructions.forEach(instruction => {
      instruction.keys.forEach(keySignerPair => {
        allKeys.push(keySignerPair);
      });

      const programId = instruction.programId.toString();
      if (!programIds.includes(programId)) {
        programIds.push(programId);
      }
    });

    allKeys.sort(function (x, y) {
      const checkSigner = x.isSigner === y.isSigner ? 0 : x.isSigner ? -1 : 1;
      const checkWritable =
        x.isWritable === y.isWritable ? 0 : x.isWritable ? -1 : 1;
      return checkSigner || checkWritable;
    });

    allKeys.forEach(keySignerPair => {
      const keyStr = keySignerPair.pubkey.toString();
      if (!keys.includes(keyStr)) {
        if (keySignerPair.isSigner) {
          this.signatures.push({
            signature: null,
            publicKey: keySignerPair.pubkey,
          });
          if (!keySignerPair.isWritable) {
            numReadonlySignedAccounts += 1;
          }
        } else {
          if (!keySignerPair.isWritable) {
            numReadonlyUnsignedAccounts += 1;
          }
        }
        keys.push(keyStr);
      }
    });

    programIds.forEach(programId => {
      if (!keys.includes(programId)) {
        keys.push(programId);
        numReadonlyUnsignedAccounts += 1;
      }
    });

    let keyCount = [];
    shortvec.encodeLength(keyCount, keys.length);

    const instructions = this.instructions.map(instruction => {
      const {data, programId} = instruction;
      let keyIndicesCount = [];
      shortvec.encodeLength(keyIndicesCount, instruction.keys.length);
      let dataCount = [];
      shortvec.encodeLength(dataCount, instruction.data.length);
      return {
        programIdIndex: keys.indexOf(programId.toString()),
        keyIndicesCount: Buffer.from(keyIndicesCount),
        keyIndices: Buffer.from(
          instruction.keys.map(keyObj =>
            keys.indexOf(keyObj.pubkey.toString()),
          ),
        ),
        dataLength: Buffer.from(dataCount),
        data,
      };
    });

    instructions.forEach(instruction => {
      invariant(instruction.programIdIndex >= 0);
      instruction.keyIndices.forEach(keyIndex => invariant(keyIndex >= 0));
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
      BufferLayout.seq(Layout.publicKey('key'), keys.length, 'keys'),
      Layout.publicKey('recentBlockhash'),
    ]);

    const transaction = {
      numRequiredSignatures: Buffer.from([this.signatures.length]),
      numReadonlySignedAccounts: Buffer.from([numReadonlySignedAccounts]),
      numReadonlyUnsignedAccounts: Buffer.from([numReadonlyUnsignedAccounts]),
      keyCount: Buffer.from(keyCount),
      keys: keys.map(key => new PublicKey(key).toBuffer()),
      recentBlockhash: Buffer.from(bs58.decode(recentBlockhash)),
    };

    let signData = Buffer.alloc(2048);
    const length = signDataLayout.encode(transaction, signData);
    instructionBuffer.copy(signData, length);
    signData = signData.slice(0, length + instructionBuffer.length);

    return signData;
  }

  /**
   * Sign the Transaction with the specified accounts.  Multiple signatures may
   * be applied to a Transaction. The first signature is considered "primary"
   * and is used when testing for Transaction confirmation.
   *
   * Transaction fields should not be modified after the first call to `sign`,
   * as doing so may invalidate the signature and cause the Transaction to be
   * rejected.
   *
   * The Transaction must be assigned a valid `recentBlockhash` before invoking this method
   */
  sign(...signers: Array<Account>) {
    this.signPartial(...signers);
  }

  /**
   * Partially sign a Transaction with the specified accounts.  The `Account`
   * inputs will be used to sign the Transaction immediately, while any
   * `PublicKey` inputs will be referenced in the signed Transaction but need to
   * be filled in later by calling `addSigner()` with the matching `Account`.
   *
   * All the caveats from the `sign` method apply to `signPartial`
   */
  signPartial(...partialSigners: Array<PublicKey | Account>) {
    if (partialSigners.length === 0) {
      throw new Error('No signers');
    }
    const signatures: Array<SignaturePubkeyPair> = partialSigners.map(
      accountOrPublicKey => {
        const publicKey =
          accountOrPublicKey instanceof Account
            ? accountOrPublicKey.publicKey
            : accountOrPublicKey;
        return {
          signature: null,
          publicKey,
        };
      },
    );
    this.signatures = signatures;
    const signData = this.signData;

    partialSigners.forEach((accountOrPublicKey, index) => {
      if (accountOrPublicKey instanceof PublicKey) {
        return;
      }
      const signature = nacl.sign.detached(
        signData,
        accountOrPublicKey.secretKey,
      );
      invariant(signature.length === 64);
      signatures[index].signature = Buffer.from(signature);
    });
  }

  /**
   * Fill in a signature for a partially signed Transaction.  The `signer` must
   * be the corresponding `Account` for a `PublicKey` that was previously provided to
   * `signPartial`
   */
  addSigner(signer: Account) {
    const index = this.signatures.findIndex(sigpair =>
      signer.publicKey.equals(sigpair.publicKey),
    );
    if (index < 0) {
      throw new Error(`Unknown signer: ${signer.publicKey.toString()}`);
    }

    const signData = this.signData;
    const signature = nacl.sign.detached(signData, signer.secretKey);
    invariant(signature.length === 64);
    this.signatures[index].signature = Buffer.from(signature);
  }

  /**
   * Verify signatures of a complete, signed Transaction
   */
  verifySignatures(): boolean {
    let verified = true;
    const signData = this.signData;
    for (const {signature, publicKey} of this.signatures) {
      if (
        !nacl.sign.detached.verify(signData, signature, publicKey.toBuffer())
      ) {
        verified = false;
      }
    }
    return verified;
  }

  /**
   * Serialize the Transaction in the wire format.
   *
   * The Transaction must have a valid `signature` before invoking this method
   */
  serialize(): Buffer {
    const {signatures} = this;
    if (!signatures) {
      throw new Error('Transaction has not been signed');
    }

    const signData = this.signData;
    const signatureCount = [];
    shortvec.encodeLength(signatureCount, signatures.length);
    const transactionLength =
      signatureCount.length + signatures.length * 64 + signData.length;
    const wireTransaction = Buffer.alloc(transactionLength);
    invariant(signatures.length < 256);
    Buffer.from(signatureCount).copy(wireTransaction, 0);
    signatures.forEach(({signature}, index) => {
      if (signature !== null) {
        invariant(signature.length === 64, `signature has invalid length`);
        Buffer.from(signature).copy(
          wireTransaction,
          signatureCount.length + index * 64,
        );
      }
    });
    signData.copy(
      wireTransaction,
      signatureCount.length + signatures.length * 64,
    );
    invariant(
      wireTransaction.length <= PACKET_DATA_SIZE,
      `Transaction too large: ${wireTransaction.length} > ${PACKET_DATA_SIZE}`,
    );
    return wireTransaction;
  }

  /**
   * Deprecated method
   * @private
   */
  get keys(): Array<PublicKey> {
    invariant(this.instructions.length === 1);
    return this.instructions[0].keys.map(keyObj => keyObj.pubkey);
  }

  /**
   * Deprecated method
   * @private
   */
  get programId(): PublicKey {
    invariant(this.instructions.length === 1);
    return this.instructions[0].programId;
  }

  /**
   * Deprecated method
   * @private
   */
  get data(): Buffer {
    invariant(this.instructions.length === 1);
    return this.instructions[0].data;
  }

  /**
   * Parse a wire transaction into a Transaction object.
   */
  static from(buffer: Buffer | Uint8Array | Array<number>): Transaction {
    // Slice up wire data
    let byteArray = [...buffer];

    const signatureCount = shortvec.decodeLength(byteArray);
    let signatures = [];
    for (let i = 0; i < signatureCount; i++) {
      const signature = byteArray.slice(0, SIGNATURE_LENGTH);
      byteArray = byteArray.slice(SIGNATURE_LENGTH);
      signatures.push(bs58.encode(Buffer.from(signature)));
    }

    const numRequiredSignatures = byteArray.shift();
    // byteArray = byteArray.slice(1); // Skip numRequiredSignatures byte
    const numReadonlySignedAccounts = byteArray.shift();
    // byteArray = byteArray.slice(1); // Skip numReadonlySignedAccounts byte
    const numReadonlyUnsignedAccounts = byteArray.shift();
    // byteArray = byteArray.slice(1); // Skip numReadonlyUnsignedAccounts byte

    const accountCount = shortvec.decodeLength(byteArray);
    let accounts = [];
    for (let i = 0; i < accountCount; i++) {
      const account = byteArray.slice(0, PUBKEY_LENGTH);
      byteArray = byteArray.slice(PUBKEY_LENGTH);
      accounts.push(bs58.encode(Buffer.from(account)));
    }

    const recentBlockhash = byteArray.slice(0, PUBKEY_LENGTH);
    byteArray = byteArray.slice(PUBKEY_LENGTH);

    const instructionCount = shortvec.decodeLength(byteArray);
    let instructions = [];
    for (let i = 0; i < instructionCount; i++) {
      let instruction = {};
      instruction.programIdIndex = byteArray.shift();
      const accountCount = shortvec.decodeLength(byteArray);
      instruction.accounts = byteArray.slice(0, accountCount);
      byteArray = byteArray.slice(accountCount);
      const dataLength = shortvec.decodeLength(byteArray);
      const data = byteArray.slice(0, dataLength);
      instruction.data = bs58.encode(Buffer.from(data));
      byteArray = byteArray.slice(dataLength);
      instructions.push(instruction);
    }

    return Transaction._populate(
      signatures,
      accounts,
      instructions,
      recentBlockhash,
      numRequiredSignatures,
      numReadonlySignedAccounts,
      numReadonlyUnsignedAccounts,
    );
  }

  /**
   * Parse an RPC result into a Transaction object.
   */
  static fromRpcResult(rpcResult: any): Transaction {
    const signatures = rpcResult.signatures;
    const accounts = rpcResult.message.accountKeys;
    const instructions = rpcResult.message.instructions;
    const recentBlockhash = rpcResult.message.recentBlockhash;
    const numRequiredSignatures =
      rpcResult.message.header.numRequiredSignatures;
    const numReadonlySignedAccounts =
      rpcResult.message.header.numReadonlySignedAccounts;
    const numReadonlyUnsignedAccounts =
      rpcResult.message.header.numReadonlyUnsignedAccounts;
    return Transaction._populate(
      signatures,
      accounts,
      instructions,
      recentBlockhash,
      numRequiredSignatures,
      numReadonlySignedAccounts,
      numReadonlyUnsignedAccounts,
    );
  }

  /**
   * Populate Transaction object
   * @private
   */
  static _populate(
    signatures: Array<string>,
    accounts: Array<string>,
    instructions: Array<any>,
    recentBlockhash: Array<number>,
    numRequiredSignatures: number,
    numReadonlySignedAccounts: number,
    numReadonlyUnsignedAccounts: number,
  ): Transaction {
    function isWritable(
      i: number,
      numRequiredSignatures: number,
      numReadonlySignedAccounts: number,
      numReadonlyUnsignedAccounts: number,
      numKeys: number,
    ): boolean {
      return (
        i < numRequiredSignatures - numReadonlySignedAccounts ||
        (i >= numRequiredSignatures &&
          i < numKeys - numReadonlyUnsignedAccounts)
      );
    }

    const transaction = new Transaction();
    transaction.recentBlockhash = new PublicKey(recentBlockhash).toBase58();
    for (let i = 0; i < signatures.length; i++) {
      const sigPubkeyPair = {
        signature:
          signatures[i] == bs58.encode(DEFAULT_SIGNATURE)
            ? null
            : bs58.decode(signatures[i]),
        publicKey: new PublicKey(accounts[i]),
      };
      transaction.signatures.push(sigPubkeyPair);
    }
    for (let i = 0; i < instructions.length; i++) {
      let instructionData = {
        keys: [],
        programId: new PublicKey(accounts[instructions[i].programIdIndex]),
        data: bs58.decode(instructions[i].data),
      };
      for (let j = 0; j < instructions[i].accounts.length; j++) {
        const pubkey = new PublicKey(accounts[instructions[i].accounts[j]]);

        instructionData.keys.push({
          pubkey,
          isSigner: transaction.signatures.some(
            keyObj => keyObj.publicKey.toString() === pubkey.toString(),
          ),
          isWritable: isWritable(
            j,
            numRequiredSignatures,
            numReadonlySignedAccounts,
            numReadonlyUnsignedAccounts,
            accounts.length,
          ),
        });
      }
      let instruction = new TransactionInstruction(instructionData);
      transaction.instructions.push(instruction);
    }
    return transaction;
  }
}
