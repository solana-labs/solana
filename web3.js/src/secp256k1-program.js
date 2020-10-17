// @flow

import * as BufferLayout from 'buffer-layout';
import {publicKeyCreate, ecdsaSign, publicKeyConvert} from 'secp256k1';
import createKeccakHash from 'keccak';
import {PublicKey} from './publickey';
import {Transaction, TransactionInstruction} from './transaction';
import {toBuffer} from './util/to-buffer';
import assert from 'assert';
import {encodeData, decodeData} from './instruction';

const PRIVATE_KEY_BYTES = 32;
const PUBLIC_KEY_BYTES = 65;
const HASHED_PUBKEY_SERIALIZED_SIZE = 20;
const SIGNATURE_OFFSETS_SERIALIZED_SIZE = 11;

/**
 * Create a secp256k1 instruction using a public key params
 * @typedef {Object} CreateSecpInstructionWithPublicKeyParams
 * @property {Buffer | Uint8Array | Array<number>} publicKey
 * @property {Buffer | Uint8Array | Array<number>} message
 * @property {Buffer | Uint8Array | Array<number>} signature
 * @property {number} recoveryId
 */
export type CreateSecpInstructionWithPublicKeyParams = {|
  publicKey: Buffer | Uint8Array | Array<number>,
  message: Buffer | Uint8Array | Array<number>,
  signature: Buffer | Uint8Array | Array<number>,
  recoveryId: number,
|};

/**
 * Create a secp256k1 instruction using a private key params
 * @typedef {Object} CreateSecpInstructionWithPrivateKeyParams
 * @property {Buffer | Uint8Array | Array<number>} privateKey
 * @property {Buffer | Uint8Array | Array<number>} message
 */
export type CreateSecpInstructionWithPrivateKeyParams = {|
  privateKey: Buffer | Uint8Array | Array<number>,
  message: Buffer | Uint8Array | Array<number>,
|};

/**
 * A decoded Secp256k instruction
 * @typedef {Object} DecodedSecp256kInstruction
 * @property {Buffer} signature
 * @property {Buffer} ethPublicKey
 * @property {Buffer} message
 * @property {number} recoveryId
 */
export type DecodedSecp256kInstruction = {|
  signature: Buffer,
  ethPublicKey: Buffer,
  message: Buffer,
  recoveryId: number,
|};

export class Secp256k1Instruction {
  /**
   * Decode a secp256k1 instruction
   */
  static decodeInstruction(instruction: TransactionInstruction) {
    const layout = BufferLayout.struct([
      BufferLayout.blob(12, 'offsetData'),
      BufferLayout.blob(20, 'ethPublicKey'),
      BufferLayout.blob(64, 'signature'),
      BufferLayout.u8('recoveryId'),
    ]);

    const {signature, ethPublicKey, recoveryId} = layout.decode(
      instruction.data,
    );
    const message = instruction.data.slice(97);

    return {
      signature,
      ethPublicKey,
      recoveryId,
      message,
    };
  }

  /**
   * @private
   */
  static checkProgramId(programId: PublicKey) {
    if (!programId.equals(Secp256kProgram.programId)) {
      throw new Error('invalid instruction; programId is not Secp256kProgram');
    }
  }
}

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
  createSecpInstructionWithPublicKey(
    params: CreateSecpInstructionWithPublicKeyParams,
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

    const instructionData = buildU8intArray([
      new Uint8Array([numSignatures]),
      buildUint8ArrayFromUint16Array([signatureOffset]),
      new Uint8Array([0]),
      buildUint8ArrayFromUint16Array([ethAddressOffset]),
      new Uint8Array([0]),
      buildUint8ArrayFromUint16Array([messageDataOffset]),
      buildUint8ArrayFromUint16Array([message.length]),
      new Uint8Array([0]),
      ethPublicKey,
      signature,
      new Uint8Array([recoveryId]),
      message,
    ]);

    return new TransactionInstruction({
      keys: [],
      programId: Secp256kProgram.programId,
      data: toBuffer(instructionData),
    });
  }

  /**
   * Create a secp256k1 instruction with private key
   */
  createSecpInstructionWithPrivateKey(
    params: CreateSecpInstructionWithPrivateKeyParams,
  ): TransactionInstruction {
    const {privateKey, message} = params;

    assert(
      privateKey.length === PRIVATE_KEY_BYTES,
      `Private key must be ${PRIVATE_KEY_BYTES} bytes`,
    );

    try {
      const publicKey = publicKeyCreate(privateKey, false);
      const messageHash = createKeccakHash('keccak256')
        .update(toBuffer(message))
        .digest();
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);

      return this.createSecpInstructionWithPublicKey({
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

export function constructEthPubkey(
  publicKey: Buffer | Uint8Array | Array<number>,
): Buffer {
  return createKeccakHash('keccak256')
    .update(toBuffer(publicKey.slice(1))) // throw away leading byte
    .digest()
    .slice(-HASHED_PUBKEY_SERIALIZED_SIZE);
}

function buildU8intArray(dataArrays): Uint8Array {
  const size = dataArrays.reduce((acc, cur) => acc + cur.length, 0);

  const final = new Uint8Array(size);

  let pos = 0;
  dataArrays.forEach(item => {
    final.set(item, pos);
    pos += item.length;
  });

  return final;
}

function buildUint8ArrayFromUint16Array(array: Array<number>): Uint8Array {
  const uint16 = new Uint16Array(array);
  return new Uint8Array(uint16.buffer, uint16.byteOffset, uint16.byteLength);
}
