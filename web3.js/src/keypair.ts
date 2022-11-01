import {generateKeypair, getPublicKey, Ed25519Keypair} from './utils/ed25519';
import {PublicKey} from './publickey';

/**
 * Keypair signer interface
 */
export interface Signer {
  publicKey: PublicKey;
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
    this._keypair = keypair ?? generateKeypair();
  }

  /**
   * Generate a new random keypair
   */
  static generate(): Keypair {
    return new Keypair(generateKeypair());
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
    if (secretKey.byteLength !== 64) {
      throw new Error('bad secret key size');
    }
    const publicKey = secretKey.slice(32, 64);
    if (!options || !options.skipValidation) {
      const privateScalar = secretKey.slice(0, 32);
      const computedPublicKey = getPublicKey(privateScalar);
      for (let ii = 0; ii < 32; ii++) {
        if (publicKey[ii] !== computedPublicKey[ii]) {
          throw new Error('provided secretKey is invalid');
        }
      }
    }
    return new Keypair({publicKey, secretKey});
  }

  /**
   * Generate a keypair from a 32 byte seed.
   *
   * @param seed seed byte array
   */
  static fromSeed(seed: Uint8Array): Keypair {
    const publicKey = getPublicKey(seed);
    const secretKey = new Uint8Array(64);
    secretKey.set(seed);
    secretKey.set(publicKey, 32);
    return new Keypair({publicKey, secretKey});
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
    return new Uint8Array(this._keypair.secretKey);
  }
}
