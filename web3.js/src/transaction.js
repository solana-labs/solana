// @flow

import assert from 'assert';
import * as BufferLayout from 'buffer-layout';
import nacl from 'tweetnacl';
import bs58 from 'bs58';

import * as Layout from './layout';
import type {Account} from './account';
import type {PublicKey} from './publickey';

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
  programId: PublicKey;

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
    const {lastId, keys, programId, userdata} = this;
    if (!lastId) {
      throw new Error('Transaction lastId required');
    }

    const signDataLayout = BufferLayout.struct([
      BufferLayout.ns64('keysLength'),
      BufferLayout.seq(
        Layout.publicKey('key'),
        keys.length,
        'keys'
      ),
      Layout.publicKey('programId'),
      Layout.publicKey('lastId'),
      BufferLayout.ns64('fee'),
      BufferLayout.ns64('userdataLength'),
      BufferLayout.blob(userdata.length, 'userdata'),
    ]);

    let signData = Buffer.alloc(2048);
    let length = signDataLayout.encode(
      {
        keysLength: keys.length,
        keys: keys.map((key) => key.toBuffer()),
        programId: programId.toBuffer(),
        lastId: Buffer.from(bs58.decode(lastId)),
        fee: 0,
        userdataLength: userdata.length,
        userdata,
      },
      signData
    );

    if (userdata.length === 0) {
      // If userdata is empty, strip the 64bit 'userdataLength' field from
      // the end of signData
      length -= 8;
    }
    signData = signData.slice(0, length);
    return signData;
  }

  /**
   * Sign the Transaction with the specified account
   *
   * The Transaction must be assigned a valid `lastId` before invoking this method
   */
  sign(from: Account) {
    const signData = this._getSignData();
    this.signature = nacl.sign.detached(signData, from.secretKey);
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

    const signData = this._getSignData();
    const wireTransaction = Buffer.alloc(
      signature.length + signData.length
    );

    Buffer.from(signature).copy(wireTransaction, 0);
    signData.copy(wireTransaction, signature.length);
    return wireTransaction;
  }
}

