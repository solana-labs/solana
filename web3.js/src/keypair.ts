import * as nacl from 'tweetnacl';

import {PublicKey} from './publickey';

/**
 * Keypair signer interface
 */
export interface Signer {
  publicKey: PublicKey;
  secretKey: Uint8Array;
}

/**
 * Ed25519 Keypair
 */
export interface Ed25519Keypair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

/**
 * An account keypair used for signing transactions.
 */
export class Keypair {
  private _keypair: Ed25519Keypair;

  /**
   * Create a new keypair instance.
   * Generate random keypair if no {@link Ed25519Keypair} is provided.
   *
   * @param keypair ed25519 keypair
   */
  constructor(keypair?: Ed25519Keypair) {
    if (keypair) {
      this._keypair = keypair;
    } else {
      this._keypair = nacl.sign.keyPair();
    }
  }

  /**
   * Generate a new random keypair
   */
  static generate(): Keypair {
    return new Keypair(nacl.sign.keyPair());
  }

  /**
   * Create a keypair from a raw secret key byte array.
   *
   * This method should only be used to recreate a keypair from a previously
   * generated secret key. Generating keypairs from a random seed should be done
   * with the {@link Keypair.fromSeed} method.
   *
   * @throws error if the provided secret key is invalid and validation is not skipped.
   *
   * @param secretKey secret key byte array
   * @param options: skip secret key validation
   */
  static fromSecretKey(
    secretKey: Uint8Array,
    options?: {skipValidation?: boolean},
  ): Keypair {
    const keypair = nacl.sign.keyPair.fromSecretKey(secretKey);
    if (!options || !options.skipValidation) {
      const encoder = new TextEncoder();
      const signData = encoder.encode('@solana/web3.js-validation-v1');
      const signature = nacl.sign.detached(signData, keypair.secretKey);
      if (!nacl.sign.detached.verify(signData, signature, keypair.publicKey)) {
        throw new Error('provided secretKey is invalid');
      }
    }
    return new Keypair(keypair);
  }

  /**
   * Generate a keypair from a 32 byte seed.
   *
   * @param seed seed byte array
   */
  static fromSeed(seed: Uint8Array): Keypair {
    return new Keypair(nacl.sign.keyPair.fromSeed(seed));
  }

  /**
   * The public key for this keypair
   */
  get publicKey(): PublicKey {
    return new PublicKey(this._keypair.publicKey);
  }

  /**
   * The raw secret key for this keypair
   */
  get secretKey(): Uint8Array {
    return this._keypair.secretKey;
  }
}
