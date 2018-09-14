// @flow

import assert from 'assert';
import nacl from 'tweetnacl';
import bs58 from 'bs58';

import type {Account, PublicKey} from './account';

/**
 * @private
 */
function bs58DecodePublicKey(key: PublicKey): Buffer {
  const keyBytes = Buffer.from(bs58.decode(key));
  assert(keyBytes.length === 32);
  return keyBytes;
}

/**
 * @typedef {string} TransactionSignature
 */
export type TransactionSignature = string;

/**
 * @typedef {string} TransactionId
 */
export type TransactionId = string;

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
   * A recent transaction id.  Must be populated by the caller
   */
  lastId: ?TransactionId;

  /**
   * Fee for this transaction
   */
  fee: number = 0;

  /**
   * Contract input userdata to include
   */
  userdata: Buffer = Buffer.alloc(0);

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
      const keyBytes = bs58DecodePublicKey(key);
      keyBytes.copy(transactionData, pos);
      pos += 32;
    }

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
}

/**
 * A Transaction that immediately transfers tokens
 */
export class TransferTokensTransaction extends Transaction {
  constructor(from: PublicKey, to: PublicKey, amount: number) {
    super();

    // Forge a simple Budget Pay contract into `userdata`
    // TODO: Clean this up
    const userdata = Buffer.alloc(68); // 68 = serialized size of Budget enum
    userdata.writeUInt32LE(60, 0);
    userdata.writeUInt32LE(amount, 12);        // u64
    userdata.writeUInt32LE(amount, 28);        // u64
    const toData = bs58DecodePublicKey(to);
    toData.copy(userdata, 36);

    Object.assign(this, {
      fee: 0,
      keys: [from, to],
      userdata
    });
  }
}
