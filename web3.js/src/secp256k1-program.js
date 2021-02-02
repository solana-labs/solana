// @flow

import {Buffer} from 'buffer';
import * as BufferLayout from 'buffer-layout';
import secp256k1 from 'secp256k1';
import assert from 'assert';
import {keccak_256} from 'js-sha3';

import {PublicKey} from './publickey';
import {TransactionInstruction} from './transaction';
import {toBuffer} from './util/to-buffer';

const {publicKeyCreate, ecdsaSign} = secp256k1;

const PRIVATE_KEY_BYTES = 32;
const PUBLIC_KEY_BYTES = 65;
const HASHED_PUBKEY_SERIALIZED_SIZE = 20;
const SIGNATURE_OFFSETS_SERIALIZED_SIZE = 11;

/**
 * Create a Secp256k1 instruction using a public key params
 * @typedef {Object} CreateSecp256k1InstructionWithPublicKeyParams
 * @property {Buffer | Uint8Array | Array<number>} publicKey
 * @property {Buffer | Uint8Array | Array<number>} message
 * @property {Buffer | Uint8Array | Array<number>} signature
 * @property {number} recoveryId
 */
export type CreateSecp256k1InstructionWithPublicKeyParams = {|
  publicKey: Buffer | Uint8Array | Array<number>,
  message: Buffer | Uint8Array | Array<number>,
  signature: Buffer | Uint8Array | Array<number>,
  recoveryId: number,
|};

/**
 * Create a Secp256k1 instruction using a private key params
 * @typedef {Object} CreateSecp256k1InstructionWithPrivateKeyParams
 * @property {Buffer | Uint8Array | Array<number>} privateKey
 * @property {Buffer | Uint8Array | Array<number>} message
 */
export type CreateSecp256k1InstructionWithPrivateKeyParams = {|
  privateKey: Buffer | Uint8Array | Array<number>,
  message: Buffer | Uint8Array | Array<number>,
|};

const SECP256K1_INSTRUCTION_LAYOUT = BufferLayout.struct([
  BufferLayout.u8('numSignatures'),
  BufferLayout.u16('signatureOffset'),
  BufferLayout.u8('signatureInstructionIndex'),
  BufferLayout.u16('ethAddressOffset'),
  BufferLayout.u8('ethAddressInstructionIndex'),
  BufferLayout.u16('messageDataOffset'),
  BufferLayout.u16('messageDataSize'),
  BufferLayout.u8('messageInstructionIndex'),
  BufferLayout.blob(20, 'ethPublicKey'),
  BufferLayout.blob(64, 'signature'),
  BufferLayout.u8('recoveryId'),
]);

export class Secp256k1Program {
  /**
   * Public key that identifies the Secp256k program
   */
  static get programId(): PublicKey {
    return new PublicKey('KeccakSecp256k11111111111111111111111111111');
  }

  /**
   * Create a secp256k1 instruction with public key
   */
  static createInstructionWithPublicKey(
    params: CreateSecp256k1InstructionWithPublicKeyParams,
  ): TransactionInstruction {
    const {publicKey, message, signature, recoveryId} = params;

    assert(
      publicKey.length === PUBLIC_KEY_BYTES,
      `Public key must be ${PUBLIC_KEY_BYTES} bytes`,
    );

    let ethPublicKey;
    try {
      ethPublicKey = constructEthPubkey(publicKey);
    } catch (error) {
      throw new Error(`Error constructing ethereum public key: ${error}`);
    }

    const dataStart = 1 + SIGNATURE_OFFSETS_SERIALIZED_SIZE;
    const ethAddressOffset = dataStart;
    const signatureOffset = dataStart + ethPublicKey.length;
    const messageDataOffset = signatureOffset + signature.length + 1;
    const numSignatures = 1;

    const instructionData = Buffer.alloc(
      SECP256K1_INSTRUCTION_LAYOUT.span + message.length,
    );

    SECP256K1_INSTRUCTION_LAYOUT.encode(
      {
        numSignatures: numSignatures,
        signatureOffset: signatureOffset,
        signatureInstructionIndex: 0,
        ethAddressOffset: ethAddressOffset,
        ethAddressInstructionIndex: 0,
        messageDataOffset: messageDataOffset,
        messageDataSize: message.length,
        messageInstructionIndex: 0,
        signature: toBuffer(signature),
        ethPublicKey: ethPublicKey,
        recoveryId: recoveryId,
      },
      instructionData,
    );

    instructionData.fill(toBuffer(message), SECP256K1_INSTRUCTION_LAYOUT.span);

    return new TransactionInstruction({
      keys: [],
      programId: Secp256k1Program.programId,
      data: instructionData,
    });
  }

  /**
   * Create a secp256k1 instruction with private key
   */
  static createInstructionWithPrivateKey(
    params: CreateSecp256k1InstructionWithPrivateKeyParams,
  ): TransactionInstruction {
    const {privateKey, message} = params;

    assert(
      privateKey.length === PRIVATE_KEY_BYTES,
      `Private key must be ${PRIVATE_KEY_BYTES} bytes`,
    );

    try {
      const publicKey = publicKeyCreate(privateKey, false);
      const messageHash = Buffer.from(
        keccak_256.update(toBuffer(message)).digest(),
      );
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);

      return this.createInstructionWithPublicKey({
        publicKey,
        message,
        signature,
        recoveryId,
      });
    } catch (error) {
      throw new Error(`Error creating instruction; ${error}`);
    }
  }
}

function constructEthPubkey(
  publicKey: Buffer | Uint8Array | Array<number>,
): Buffer {
  return Buffer.from(
    keccak_256
      .update(toBuffer(publicKey.slice(1))) // throw away leading byte
      .digest(),
  ).slice(-HASHED_PUBKEY_SERIALIZED_SIZE);
}
