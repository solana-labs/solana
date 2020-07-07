// @flow

import invariant from 'assert';
import nacl from 'tweetnacl';
import bs58 from 'bs58';

import type {CompiledInstruction} from './message';
import {Message} from './message';
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
 * Account metadata used to define instructions
 *
 * @typedef {Object} AccountMeta
 * @property {PublicKey} pubkey An account's public key
 * @property {boolean} isSigner True if an instruction requires a transaction signature matching `pubkey`
 * @property {boolean} isWritable True if the `pubkey` can be loaded as a read-write account.
 */
export type AccountMeta = {
  pubkey: PublicKey,
  isSigner: boolean,
  isWritable: boolean,
};

/**
 * List of TransactionInstruction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionInstructionCtorFields
 * @property {?Array<PublicKey>} keys
 * @property {?PublicKey} programId
 * @property {?Buffer} data
 */
export type TransactionInstructionCtorFields = {|
  keys?: Array<AccountMeta>,
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
  keys: Array<AccountMeta> = [];

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
 * @property {?Blockhash} recentBlockhash A recent blockhash
 * @property {?Array<SignaturePubkeyPair>} signatures One or more signatures
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
 * @property {Blockhash} nonce The current Nonce blockhash
 * @property {TransactionInstruction} nonceInstruction AdvanceNonceAccount Instruction
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
   * Compile transaction data
   */
  compileMessage(): Message {
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

    let numReadonlySignedAccounts = 0;
    let numReadonlyUnsignedAccounts = 0;

    const accountKeys = this.signatures.map(({publicKey}) =>
      publicKey.toString(),
    );
    const programIds: string[] = [];
    const accountMetas: AccountMeta[] = [];
    this.instructions.forEach(instruction => {
      instruction.keys.forEach(accountMeta => {
        accountMetas.push(accountMeta);
      });

      const programId = instruction.programId.toString();
      if (!programIds.includes(programId)) {
        programIds.push(programId);
      }
    });

    accountMetas.sort(function (x, y) {
      const checkSigner = x.isSigner === y.isSigner ? 0 : x.isSigner ? -1 : 1;
      const checkWritable =
        x.isWritable === y.isWritable ? 0 : x.isWritable ? -1 : 1;
      return checkSigner || checkWritable;
    });

    accountMetas.forEach(({pubkey, isSigner, isWritable}) => {
      const keyStr = pubkey.toString();
      if (!accountKeys.includes(keyStr)) {
        accountKeys.push(keyStr);
        if (isSigner) {
          this.signatures.push({
            signature: null,
            publicKey: pubkey,
          });
          if (!isWritable) {
            numReadonlySignedAccounts += 1;
          }
        } else if (!isWritable) {
          numReadonlyUnsignedAccounts += 1;
        }
      }
    });

    programIds.forEach(programId => {
      if (!accountKeys.includes(programId)) {
        accountKeys.push(programId);
        numReadonlyUnsignedAccounts += 1;
      }
    });

    const instructions: CompiledInstruction[] = this.instructions.map(
      instruction => {
        const {data, programId} = instruction;
        return {
          programIdIndex: accountKeys.indexOf(programId.toString()),
          accounts: instruction.keys.map(keyObj =>
            accountKeys.indexOf(keyObj.pubkey.toString()),
          ),
          data: bs58.encode(data),
        };
      },
    );

    instructions.forEach(instruction => {
      invariant(instruction.programIdIndex >= 0);
      instruction.accounts.forEach(keyIndex => invariant(keyIndex >= 0));
    });

    return new Message({
      header: {
        numRequiredSignatures: this.signatures.length,
        numReadonlySignedAccounts,
        numReadonlyUnsignedAccounts,
      },
      accountKeys,
      recentBlockhash,
      instructions,
    });
  }

  /**
   * Get a buffer of the Transaction data that need to be covered by signatures
   */
  serializeMessage(): Buffer {
    return this.compileMessage().serialize();
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
    const signData = this.serializeMessage();

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
    const signData = this.serializeMessage();
    const signature = nacl.sign.detached(signData, signer.secretKey);
    this.addSignature(signer.publicKey, signature);
  }

  /**
   * Add an externally created signature to a transaction
   */
  addSignature(pubkey: PublicKey, signature: Buffer) {
    invariant(signature.length === 64);

    const index = this.signatures.findIndex(sigpair =>
      pubkey.equals(sigpair.publicKey),
    );
    if (index < 0) {
      throw new Error(`Unknown signer: ${pubkey.toString()}`);
    }

    this.signatures[index].signature = Buffer.from(signature);
  }

  /**
   * Verify signatures of a complete, signed Transaction
   */
  verifySignatures(): boolean {
    let verified = true;
    const signData = this.serializeMessage();
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
    if (!signatures || signatures.length === 0 || !this.verifySignatures()) {
      throw new Error('Transaction has not been signed');
    }

    const signData = this.serializeMessage();
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
    const numReadonlySignedAccounts = byteArray.shift();
    const numReadonlyUnsignedAccounts = byteArray.shift();

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

    return Transaction.populate(new Message(messageArgs), signatures);
  }

  /**
   * Populate Transaction object from message and signatures
   */
  static populate(message: Message, signatures: Array<string>): Transaction {
    const transaction = new Transaction();
    transaction.recentBlockhash = message.recentBlockhash;
    signatures.forEach((signature, index) => {
      const sigPubkeyPair = {
        signature:
          signature == bs58.encode(DEFAULT_SIGNATURE)
            ? null
            : bs58.decode(signature),
        publicKey: message.accountKeys[index],
      };
      transaction.signatures.push(sigPubkeyPair);
    });

    message.instructions.forEach(instruction => {
      const keys = instruction.accounts.map(account => {
        const pubkey = message.accountKeys[account];
        return {
          pubkey,
          isSigner: transaction.signatures.some(
            keyObj => keyObj.publicKey.toString() === pubkey.toString(),
          ),
          isWritable: message.isAccountWritable(account),
        };
      });

      transaction.instructions.push(
        new TransactionInstruction({
          keys,
          programId: message.accountKeys[instruction.programIdIndex],
          data: bs58.decode(instruction.data),
        }),
      );
    });

    return transaction;
  }
}
