import invariant from 'assert';
import nacl from 'tweetnacl';
import bs58 from 'bs58';
import {Buffer} from 'buffer';

import type {CompiledInstruction} from './message';
import {Message} from './message';
import {PublicKey} from './publickey';
import {Account} from './account';
import * as shortvec from './util/shortvec-encoding';
import type {Blockhash} from './blockhash';
import {toBuffer} from './util/to-buffer';

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
  pubkey: PublicKey;
  isSigner: boolean;
  isWritable: boolean;
};

/**
 * List of TransactionInstruction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionInstructionCtorFields
 * @property {Array<PublicKey>} keys
 * @property {PublicKey} programId
 * @property {?Buffer} data
 */
export type TransactionInstructionCtorFields = {
  keys: Array<AccountMeta>;
  programId: PublicKey;
  data?: Buffer;
};

/**
 * Configuration object for Transaction.serialize()
 *
 * @typedef {Object} SerializeConfig
 * @property {boolean|undefined} requireAllSignatures Require all transaction signatures be present (default: true)
 * @property {boolean|undefined} verifySignatures Verify provided signatures (default: true)
 */
export type SerializeConfig = {
  requireAllSignatures?: boolean;
  verifySignatures?: boolean;
};

/**
 * Transaction Instruction class
 */
export class TransactionInstruction {
  /**
   * Public keys to include in this transaction
   * Boolean represents whether this pubkey needs to sign the transaction
   */
  keys: Array<AccountMeta>;

  /**
   * Program Id to execute
   */
  programId: PublicKey;

  /**
   * Program input
   */
  data: Buffer = Buffer.alloc(0);

  constructor(opts: TransactionInstructionCtorFields) {
    this.programId = opts.programId;
    this.keys = opts.keys;
    if (opts.data) {
      this.data = opts.data;
    }
  }
}

/**
 * Pair of signature and corresponding public key
 */
export type SignaturePubkeyPair = {
  signature: Buffer | null;
  publicKey: PublicKey;
};

/**
 * List of Transaction object fields that may be initialized at construction
 *
 * @typedef {Object} TransactionCtorFields
 * @property {?Blockhash} recentBlockhash A recent blockhash
 * @property {?PublicKey} feePayer The transaction fee payer
 * @property {?Array<SignaturePubkeyPair>} signatures One or more signatures
 *
 */
type TransactionCtorFields = {
  recentBlockhash?: Blockhash | null;
  nonceInfo?: NonceInformation | null;
  feePayer?: PublicKey | null;
  signatures?: Array<SignaturePubkeyPair>;
};

/**
 * NonceInformation to be used to build a Transaction.
 *
 * @typedef {Object} NonceInformation
 * @property {Blockhash} nonce The current Nonce blockhash
 * @property {TransactionInstruction} nonceInstruction AdvanceNonceAccount Instruction
 */
type NonceInformation = {
  nonce: Blockhash;
  nonceInstruction: TransactionInstruction;
};

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
   * The transaction fee payer
   */
  feePayer?: PublicKey;

  /**
   * The instructions to atomically execute
   */
  instructions: Array<TransactionInstruction> = [];

  /**
   * A recent transaction id. Must be populated by the caller
   */
  recentBlockhash?: Blockhash;

  /**
   * Optional Nonce information. If populated, transaction will use a durable
   * Nonce hash instead of a recentBlockhash. Must be populated by the caller
   */
  nonceInfo?: NonceInformation;

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
      Transaction | TransactionInstruction | TransactionInstructionCtorFields
    >
  ): Transaction {
    if (items.length === 0) {
      throw new Error('No instructions');
    }

    items.forEach((item: any) => {
      if ('instructions' in item) {
        this.instructions = this.instructions.concat(item.instructions);
      } else if ('data' in item && 'programId' in item && 'keys' in item) {
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

    let feePayer: PublicKey;
    if (this.feePayer) {
      feePayer = this.feePayer;
    } else if (this.signatures.length > 0 && this.signatures[0].publicKey) {
      // Use implicit fee payer
      feePayer = this.signatures[0].publicKey;
    } else {
      throw new Error('Transaction fee payer required');
    }

    for (let i = 0; i < this.instructions.length; i++) {
      if (this.instructions[i].programId === undefined) {
        throw new Error(
          `Transaction instruction index ${i} has undefined program id`,
        );
      }
    }

    const programIds: string[] = [];
    const accountMetas: AccountMeta[] = [];
    this.instructions.forEach(instruction => {
      instruction.keys.forEach(accountMeta => {
        accountMetas.push({...accountMeta});
      });

      const programId = instruction.programId.toString();
      if (!programIds.includes(programId)) {
        programIds.push(programId);
      }
    });

    // Append programID account metas
    programIds.forEach(programId => {
      accountMetas.push({
        pubkey: new PublicKey(programId),
        isSigner: false,
        isWritable: false,
      });
    });

    // Sort. Prioritizing first by signer, then by writable
    accountMetas.sort(function (x, y) {
      const checkSigner = x.isSigner === y.isSigner ? 0 : x.isSigner ? -1 : 1;
      const checkWritable =
        x.isWritable === y.isWritable ? 0 : x.isWritable ? -1 : 1;
      return checkSigner || checkWritable;
    });

    // Cull duplicate account metas
    const uniqueMetas: AccountMeta[] = [];
    accountMetas.forEach(accountMeta => {
      const pubkeyString = accountMeta.pubkey.toString();
      const uniqueIndex = uniqueMetas.findIndex(x => {
        return x.pubkey.toString() === pubkeyString;
      });
      if (uniqueIndex > -1) {
        uniqueMetas[uniqueIndex].isWritable =
          uniqueMetas[uniqueIndex].isWritable || accountMeta.isWritable;
      } else {
        uniqueMetas.push(accountMeta);
      }
    });

    // Move fee payer to the front
    const feePayerIndex = uniqueMetas.findIndex(x => {
      return x.pubkey.equals(feePayer);
    });
    if (feePayerIndex > -1) {
      const [payerMeta] = uniqueMetas.splice(feePayerIndex, 1);
      payerMeta.isSigner = true;
      payerMeta.isWritable = true;
      uniqueMetas.unshift(payerMeta);
    } else {
      uniqueMetas.unshift({
        pubkey: feePayer,
        isSigner: true,
        isWritable: true,
      });
    }

    // Disallow unknown signers
    for (const signature of this.signatures) {
      const uniqueIndex = uniqueMetas.findIndex(x => {
        return x.pubkey.equals(signature.publicKey);
      });
      if (uniqueIndex > -1) {
        if (!uniqueMetas[uniqueIndex].isSigner) {
          uniqueMetas[uniqueIndex].isSigner = true;
          console.warn(
            'Transaction references a signature that is unnecessary, ' +
              'only the fee payer and instruction signer accounts should sign a transaction. ' +
              'This behavior is deprecated and will throw an error in the next major version release.',
          );
        }
      } else {
        throw new Error(`unknown signer: ${signature.publicKey.toString()}`);
      }
    }

    let numRequiredSignatures = 0;
    let numReadonlySignedAccounts = 0;
    let numReadonlyUnsignedAccounts = 0;

    // Split out signing from non-signing keys and count header values
    const signedKeys: string[] = [];
    const unsignedKeys: string[] = [];
    uniqueMetas.forEach(({pubkey, isSigner, isWritable}) => {
      if (isSigner) {
        signedKeys.push(pubkey.toString());
        numRequiredSignatures += 1;
        if (!isWritable) {
          numReadonlySignedAccounts += 1;
        }
      } else {
        unsignedKeys.push(pubkey.toString());
        if (!isWritable) {
          numReadonlyUnsignedAccounts += 1;
        }
      }
    });

    const accountKeys = signedKeys.concat(unsignedKeys);
    const instructions: CompiledInstruction[] = this.instructions.map(
      instruction => {
        const {data, programId} = instruction;
        return {
          programIdIndex: accountKeys.indexOf(programId.toString()),
          accounts: instruction.keys.map(meta =>
            accountKeys.indexOf(meta.pubkey.toString()),
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
        numRequiredSignatures,
        numReadonlySignedAccounts,
        numReadonlyUnsignedAccounts,
      },
      accountKeys,
      recentBlockhash,
      instructions,
    });
  }

  /**
   * @internal
   */
  _compile(): Message {
    const message = this.compileMessage();
    const signedKeys = message.accountKeys.slice(
      0,
      message.header.numRequiredSignatures,
    );

    if (this.signatures.length === signedKeys.length) {
      const valid = this.signatures.every((pair, index) => {
        return signedKeys[index].equals(pair.publicKey);
      });

      if (valid) return message;
    }

    this.signatures = signedKeys.map(publicKey => ({
      signature: null,
      publicKey,
    }));

    return message;
  }

  /**
   * Get a buffer of the Transaction data that need to be covered by signatures
   */
  serializeMessage(): Buffer {
    return this._compile().serialize();
  }

  /**
   * Specify the public keys which will be used to sign the Transaction.
   * The first signer will be used as the transaction fee payer account.
   *
   * Signatures can be added with either `partialSign` or `addSignature`
   *
   * @deprecated Deprecated since v0.84.0. Only the fee payer needs to be
   * specified and it can be set in the Transaction constructor or with the
   * `feePayer` property.
   */
  setSigners(...signers: Array<PublicKey>) {
    if (signers.length === 0) {
      throw new Error('No signers');
    }

    const seen = new Set();
    this.signatures = signers
      .filter(publicKey => {
        const key = publicKey.toString();
        if (seen.has(key)) {
          return false;
        } else {
          seen.add(key);
          return true;
        }
      })
      .map(publicKey => ({signature: null, publicKey}));
  }

  /**
   * Sign the Transaction with the specified accounts. Multiple signatures may
   * be applied to a Transaction. The first signature is considered "primary"
   * and is used identify and confirm transactions.
   *
   * If the Transaction `feePayer` is not set, the first signer will be used
   * as the transaction fee payer account.
   *
   * Transaction fields should not be modified after the first call to `sign`,
   * as doing so may invalidate the signature and cause the Transaction to be
   * rejected.
   *
   * The Transaction must be assigned a valid `recentBlockhash` before invoking this method
   */
  sign(...signers: Array<Account>) {
    if (signers.length === 0) {
      throw new Error('No signers');
    }

    // Dedupe signers
    const seen = new Set();
    const uniqueSigners = [];
    for (const signer of signers) {
      const key = signer.publicKey.toString();
      if (seen.has(key)) {
        continue;
      } else {
        seen.add(key);
        uniqueSigners.push(signer);
      }
    }

    this.signatures = uniqueSigners.map(signer => ({
      signature: null,
      publicKey: signer.publicKey,
    }));

    const message = this._compile();
    this._partialSign(message, ...uniqueSigners);
    this._verifySignatures(message.serialize(), true);
  }

  /**
   * Partially sign a transaction with the specified accounts. All accounts must
   * correspond to either the fee payer or a signer account in the transaction
   * instructions.
   *
   * All the caveats from the `sign` method apply to `partialSign`
   */
  partialSign(...signers: Array<Account>) {
    if (signers.length === 0) {
      throw new Error('No signers');
    }

    // Dedupe signers
    const seen = new Set();
    const uniqueSigners = [];
    for (const signer of signers) {
      const key = signer.publicKey.toString();
      if (seen.has(key)) {
        continue;
      } else {
        seen.add(key);
        uniqueSigners.push(signer);
      }
    }

    const message = this._compile();
    this._partialSign(message, ...uniqueSigners);
  }

  /**
   * @internal
   */
  _partialSign(message: Message, ...signers: Array<Account>) {
    const signData = message.serialize();
    signers.forEach(signer => {
      const signature = nacl.sign.detached(signData, signer.secretKey);
      this._addSignature(signer.publicKey, toBuffer(signature));
    });
  }

  /**
   * Add an externally created signature to a transaction. The public key
   * must correspond to either the fee payer or a signer account in the transaction
   * instructions.
   */
  addSignature(pubkey: PublicKey, signature: Buffer) {
    this._compile(); // Ensure signatures array is populated
    this._addSignature(pubkey, signature);
  }

  /**
   * @internal
   */
  _addSignature(pubkey: PublicKey, signature: Buffer) {
    invariant(signature.length === 64);

    const index = this.signatures.findIndex(sigpair =>
      pubkey.equals(sigpair.publicKey),
    );
    if (index < 0) {
      throw new Error(`unknown signer: ${pubkey.toString()}`);
    }

    this.signatures[index].signature = Buffer.from(signature);
  }

  /**
   * Verify signatures of a complete, signed Transaction
   */
  verifySignatures(): boolean {
    return this._verifySignatures(this.serializeMessage(), true);
  }

  /**
   * @internal
   */
  _verifySignatures(signData: Buffer, requireAllSignatures: boolean): boolean {
    for (const {signature, publicKey} of this.signatures) {
      if (signature === null) {
        if (requireAllSignatures) {
          return false;
        }
      } else {
        if (
          !nacl.sign.detached.verify(signData, signature, publicKey.toBytes())
        ) {
          return false;
        }
      }
    }
    return true;
  }

  /**
   * Serialize the Transaction in the wire format.
   */
  serialize(config?: SerializeConfig): Buffer {
    const {requireAllSignatures, verifySignatures} = Object.assign(
      {requireAllSignatures: true, verifySignatures: true},
      config,
    );

    const signData = this.serializeMessage();
    if (
      verifySignatures &&
      !this._verifySignatures(signData, requireAllSignatures)
    ) {
      throw new Error('Signature verification failed');
    }

    return this._serialize(signData);
  }

  /**
   * @internal
   */
  _serialize(signData: Buffer): Buffer {
    const {signatures} = this;
    const signatureCount: number[] = [];
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
   * @internal
   */
  get keys(): Array<PublicKey> {
    invariant(this.instructions.length === 1);
    return this.instructions[0].keys.map(keyObj => keyObj.pubkey);
  }

  /**
   * Deprecated method
   * @internal
   */
  get programId(): PublicKey {
    invariant(this.instructions.length === 1);
    return this.instructions[0].programId;
  }

  /**
   * Deprecated method
   * @internal
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

    return Transaction.populate(Message.from(byteArray), signatures);
  }

  /**
   * Populate Transaction object from message and signatures
   */
  static populate(message: Message, signatures: Array<string>): Transaction {
    const transaction = new Transaction();
    transaction.recentBlockhash = message.recentBlockhash;
    if (message.header.numRequiredSignatures > 0) {
      transaction.feePayer = message.accountKeys[0];
    }
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
