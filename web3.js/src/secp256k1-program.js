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
const ETHEREUM_ADDRESS_BYTES = 20;
const PUBLIC_KEY_BYTES = 64;
const SIGNATURE_OFFSETS_SERIALIZED_SIZE = 11;

/**
 * Params for creating an secp256k1 instruction using a public key
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
 * Params for creating an secp256k1 instruction using an Ethereum address
 * @typedef {Object} CreateSecp256k1InstructionWithEthAddressParams
 * @property {Buffer | Uint8Array | Array<number>} ethAddress
 * @property {Buffer | Uint8Array | Array<number>} message
 * @property {Buffer | Uint8Array | Array<number>} signature
 * @property {number} recoveryId
 */
export type CreateSecp256k1InstructionWithEthAddressParams = {|
  ethAddress: Buffer | Uint8Array | Array<number> | string,
  message: Buffer | Uint8Array | Array<number>,
  signature: Buffer | Uint8Array | Array<number>,
  recoveryId: number,
|};

/**
 * Params for creating an secp256k1 instruction using a private key
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
  BufferLayout.blob(20, 'ethAddress'),
  BufferLayout.blob(64, 'signature'),
  BufferLayout.u8('recoveryId'),
]);

export class Secp256k1Program {
  /**
   * Public key that identifies the secp256k1 program
   */
  static get programId(): PublicKey {
    return new PublicKey('KeccakSecp256k11111111111111111111111111111');
  }

  /**
   * Construct an Ethereum address from a secp256k1 public key buffer.
   * @param {Buffer} publicKey a 64 byte secp256k1 public key buffer
   */
  static publicKeyToEthAddress(
    publicKey: Buffer | Uint8Array | Array<number>,
  ): Buffer {
    assert(
      publicKey.length === PUBLIC_KEY_BYTES,
      `Public key must be ${PUBLIC_KEY_BYTES} bytes but received ${publicKey.length} bytes`,
    );

    try {
      return Buffer.from(keccak_256.update(toBuffer(publicKey)).digest()).slice(
        -ETHEREUM_ADDRESS_BYTES,
      );
    } catch (error) {
      throw new Error(`Error constructing Ethereum address: ${error}`);
    }
  }

  /**
   * Create an secp256k1 instruction with a public key. The public key
   * must be a buffer that is 64 bytes long.
   */
  static createInstructionWithPublicKey(
    params: CreateSecp256k1InstructionWithPublicKeyParams,
  ): TransactionInstruction {
    const {publicKey, message, signature, recoveryId} = params;
    return Secp256k1Program.createInstructionWithEthAddress({
      ethAddress: Secp256k1Program.publicKeyToEthAddress(publicKey),
      message,
      signature,
      recoveryId,
    });
  }

  /**
   * Create an secp256k1 instruction with an Ethereum address. The address
   * must be a hex string or a buffer that is 20 bytes long.
   */
  static createInstructionWithEthAddress(
    params: CreateSecp256k1InstructionWithEthAddressParams,
  ): TransactionInstruction {
    const {ethAddress: rawAddress, message, signature, recoveryId} = params;

    let ethAddress = rawAddress;
    if (typeof rawAddress === 'string') {
      if (rawAddress.startsWith('0x')) {
        ethAddress = Buffer.from(rawAddress.substr(2), 'hex');
      } else {
        ethAddress = Buffer.from(rawAddress, 'hex');
      }
    }

    assert(
      ethAddress.length === ETHEREUM_ADDRESS_BYTES,
      `Address must be ${ETHEREUM_ADDRESS_BYTES} bytes but received ${ethAddress.length} bytes`,
    );

    const dataStart = 1 + SIGNATURE_OFFSETS_SERIALIZED_SIZE;
    const ethAddressOffset = dataStart;
    const signatureOffset = dataStart + ethAddress.length;
    const messageDataOffset = signatureOffset + signature.length + 1;
    const numSignatures = 1;

    const instructionData = Buffer.alloc(
      SECP256K1_INSTRUCTION_LAYOUT.span + message.length,
    );

    SECP256K1_INSTRUCTION_LAYOUT.encode(
      {
        numSignatures,
        signatureOffset,
        signatureInstructionIndex: 0,
        ethAddressOffset,
        ethAddressInstructionIndex: 0,
        messageDataOffset,
        messageDataSize: message.length,
        messageInstructionIndex: 0,
        signature: toBuffer(signature),
        ethAddress: toBuffer(ethAddress),
        recoveryId,
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
   * Create an secp256k1 instruction with a private key. The private key
   * must be a buffer that is 32 bytes long.
   */
  static createInstructionWithPrivateKey(
    params: CreateSecp256k1InstructionWithPrivateKeyParams,
  ): TransactionInstruction {
    const {privateKey, message} = params;

    assert(
      privateKey.length === PRIVATE_KEY_BYTES,
      `Private key must be ${PRIVATE_KEY_BYTES} bytes but received ${privateKey.length} bytes`,
    );

    try {
      const publicKey = publicKeyCreate(privateKey, false).slice(1); // throw away leading byte
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
