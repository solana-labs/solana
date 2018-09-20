// @flow

import assert from 'assert';
import nacl from 'tweetnacl';
import bs58 from 'bs58';

import type {Account, PublicKey} from './account';

/**
 * @typedef {string} TransactionSignature
 */
export type TransactionSignature = string;

/**
 * @typedef {string} TransactionId
 */
export type TransactionId = string;

/**
 * List of Transaction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionCtorFields
 * @property {?Buffer} signature
 * @property {?Array<PublicKey>} keys
 * @property {?PublicKey} programId
 * @property {?number} fee
 * @property {?Buffer} userdata
 */
type TransactionCtorFields = {|
  signature?: Buffer;
  keys?: Array<PublicKey>;
  programId?: PublicKey;
  fee?: number;
  userdata?: Buffer;
|};

/**
 * Mirrors the Transaction struct in src/transaction.rs
 */
export class Transaction {

  /**
   * Current signature of the transaction.  Typically created by invoking the
   * `sign()` method
   */
  signature: ?Buffer;

  /**
   * Public keys to include in this transaction
   */
  keys: Array<PublicKey> = [];

  /**
   * Program Id to execute
   */
  programId: ?PublicKey;

  /**
   * A recent transaction id.  Must be populated by the caller
   */
  lastId: ?TransactionId;

  /**
   * Fee for this transaction
   */
  fee: number = 0;

  /**
   * Program input
   */
  userdata: Buffer = Buffer.alloc(0);

  constructor(opts?: TransactionCtorFields) {
    opts && Object.assign(this, opts);
  }

  /**
   * @private
   */
  _getSignData(): Buffer {
    const {lastId} = this;
    if (!lastId) {
      throw new Error('Transaction lastId required');
    }

    // Start with a Buffer that should be large enough to fit any Transaction
    const transactionData = Buffer.alloc(2048);

    let pos = 0;

    // serialize `this.keys`
    transactionData.writeUInt32LE(this.keys.length, pos);  // u64
    pos += 8;
    for (let key of this.keys) {
      const keyBytes = Transaction.serializePublicKey(key);
      keyBytes.copy(transactionData, pos);
      pos += 32;
    }

    // serialize `this.programId`
    if (this.programId) {
      const keyBytes = Transaction.serializePublicKey(this.programId);
      keyBytes.copy(transactionData, pos);
    }
    pos += 32;

    // serialize `this.lastId`
    {
      const lastIdBytes = Buffer.from(bs58.decode(lastId));
      assert(lastIdBytes.length === 32);
      lastIdBytes.copy(transactionData, pos);
      pos += 32;
    }

    // serialize `this.fee`
    transactionData.writeUInt32LE(this.fee, pos);        // u64
    pos += 8;

    // serialize `this.userdata`
    if (this.userdata.length > 0) {
      transactionData.writeUInt32LE(this.userdata.length, pos);  // u64
      pos += 8;
      this.userdata.copy(transactionData, pos);
      pos += this.userdata.length;
    }

    return transactionData.slice(0, pos);
  }

  /**
   * Sign the Transaction with the specified account
   *
   * The Transaction must be assigned a valid `lastId` before invoking this method
   */
  sign(from: Account) {
    const transactionData = this._getSignData();
    this.signature = nacl.sign.detached(transactionData, from.secretKey);
    assert(this.signature.length === 64);
  }

  /**
   * Serialize the Transaction in the wire format.
   *
   * The Transaction must have a valid `signature` before invoking this method
   */
  serialize(): Buffer {
    const {signature} = this;
    if (!signature) {
      throw new Error('Transaction has not been signed');
    }

    const transactionData = this._getSignData();
    const wireTransaction = Buffer.alloc(
      signature.length + transactionData.length
    );

    Buffer.from(signature).copy(wireTransaction, 0);
    transactionData.copy(wireTransaction, signature.length);
    return wireTransaction;
  }

  /**
   * Serializes a public key into the wire format
   */
  static serializePublicKey(key: PublicKey): Buffer {
    const data = Buffer.from(bs58.decode(key));
    assert(data.length === 32);
    return data;
  }
}

