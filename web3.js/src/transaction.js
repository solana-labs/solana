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
    const programIds = [programId];
    const instructions = [
      {
        programId: 0,
        accountsLength: keys.length,
        accounts: [...keys.keys()],
        userdataLength: userdata.length,
        userdata,
      },
    ];

    const instructionLayout = BufferLayout.struct([
      BufferLayout.u8('programId'),

      BufferLayout.u32('accountsLength'),
      BufferLayout.u32('accountsLengthPadding'),
      BufferLayout.seq(
        BufferLayout.u8('account'),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'accounts'
      ),
      BufferLayout.ns64('userdataLength'),
      BufferLayout.blob(userdata.length, 'userdata'),
    ]);

    const signDataLayout = BufferLayout.struct([
      BufferLayout.u32('accountKeysLength'),
      BufferLayout.u32('accountKeysLengthPadding'),
      BufferLayout.seq(
        Layout.publicKey('accountKey'),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'accountKeys'
      ),
      Layout.publicKey('lastId'),
      BufferLayout.ns64('fee'),

      BufferLayout.u32('programIdsLength'),
      BufferLayout.u32('programIdsLengthPadding'),
      BufferLayout.seq(
        Layout.publicKey('programId'),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'programIds'
      ),

      BufferLayout.u32('instructionsLength'),
      BufferLayout.u32('instructionsLengthPadding'),
      BufferLayout.seq(
        instructionLayout,
        BufferLayout.offset(BufferLayout.u32(), -8),
        'instructions'
      ),
    ]);

    const transaction = {
      accountKeys: keys.map((key) => key.toBuffer()),
      lastId: Buffer.from(bs58.decode(lastId)),
      fee: 0,
      programIds: programIds.map((programId) => programId.toBuffer()),
      instructions,
    };

    let signData = Buffer.alloc(2048);
    const length = signDataLayout.encode(transaction, signData);
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

