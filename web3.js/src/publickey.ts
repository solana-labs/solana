import bs58 from 'bs58';
import {Buffer} from 'buffer';
import {sha256} from '@noble/hashes/sha256';

import {isOnCurve} from './utils/ed25519';
import {Struct, SOLANA_SCHEMA} from './utils/borsh-schema';
import {toBuffer} from './utils/to-buffer';

/**
 * Maximum length of derived pubkey seed
 */
export const MAX_SEED_LENGTH = 32;

/**
 * Size of public key in bytes
 */
export const PUBLIC_KEY_LENGTH = 32;

/**
 * Value to be converted into public key
 */
export type PublicKeyInitData =
  | string
  | Buffer
  | Uint8Array
  | Array<number>
  | PublicKeyData;

/**
 * JSON object representation of PublicKey class
 */
export type PublicKeyData = {
  /** @internal */
  _buffer: Uint8Array;
};

export const byteArrayEquals = (a: Uint8Array, b: Uint8Array): boolean => {
  if (a.length !== b.length) {
    return false;
  }
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) {
      return false;
    }
  }
  return true;
};

function isPublicKeyData(value: PublicKeyInitData): value is PublicKeyData {
  return (value as PublicKeyData)._buffer !== undefined;
}

/**
 * A public key
 */
export class PublicKey extends Struct implements PublicKeyData {
  /** @internal */
  _buffer: Uint8Array;

  /**
   * Create a new PublicKey object
   * @param value ed25519 public key as buffer or base-58 encoded string
   */
  constructor(value: PublicKeyInitData) {
    super({});
    if (isPublicKeyData(value)) {
      this._buffer = value._buffer;
    } else if (typeof value === 'string') {
      // assume base 58 encoding by default
      const decoded = bs58.decode(value);
      if (decoded.length != PUBLIC_KEY_LENGTH) {
        throw new Error(`Invalid public key input`);
      }
      this._buffer = decoded;
    } else if (value instanceof Uint8Array) {
      this._buffer = value;
    } else if (Array.isArray(value)) {
      this._buffer = Uint8Array.from(value);
    } else {
      this._buffer = Uint8Array.from(value);
    }

    if (this._buffer.byteLength > PUBLIC_KEY_LENGTH) {
      throw new Error(`Invalid public key input`);
    }
  }

  /**
   * Default public key value. (All zeros)
   */
  static default: PublicKey = new PublicKey('11111111111111111111111111111111');

  /**
   * Checks if two publicKeys are equal
   */
  equals(publicKey: PublicKey): boolean {
    return byteArrayEquals(this._buffer, publicKey._buffer);
  }

  /**
   * Return the base-58 representation of the public key
   */
  toBase58(): string {
    return bs58.encode(this.toBytes());
  }

  toJSON(): string {
    return this.toBase58();
  }

  /**
   * Return the byte array representation of the public key
   */
  toBytes(): Uint8Array {
    return this._buffer.slice();
  }

  /**
   * Return the Buffer representation of the public key
   */
  toBuffer(): Buffer {
    return Buffer.from(this.toBytes());
  }

  /**
   * Return the base-58 representation of the public key
   */
  toString(): string {
    return this.toBase58();
  }

  /**
   * Derive a public key from another key, a seed, and a program ID.
   * The program ID will also serve as the owner of the public key, giving
   * it permission to write data to the account.
   */
  /* eslint-disable require-await */
  static async createWithSeed(
    fromPublicKey: PublicKey,
    seed: string,
    programId: PublicKey,
  ): Promise<PublicKey> {
    const buffer = Buffer.concat([
      fromPublicKey.toBuffer(),
      Buffer.from(seed),
      programId.toBuffer(),
    ]);
    const publicKeyBytes = sha256(buffer);
    return new PublicKey(publicKeyBytes);
  }

  /**
   * Derive a program address from seeds and a program ID.
   */
  /* eslint-disable require-await */
  static createProgramAddressSync(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): PublicKey {
    let buffer = Buffer.alloc(0);
    seeds.forEach(function (seed) {
      if (seed.length > MAX_SEED_LENGTH) {
        throw new TypeError(`Max seed length exceeded`);
      }
      buffer = Buffer.concat([buffer, toBuffer(seed)]);
    });
    buffer = Buffer.concat([
      buffer,
      programId.toBuffer(),
      Buffer.from('ProgramDerivedAddress'),
    ]);
    const publicKeyBytes = sha256(buffer);
    if (isOnCurve(publicKeyBytes)) {
      throw new Error(`Invalid seeds, address must fall off the curve`);
    }
    return new PublicKey(publicKeyBytes);
  }

  /**
   * Async version of createProgramAddressSync
   * For backwards compatibility
   */
  /* eslint-disable require-await */
  static async createProgramAddress(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): Promise<PublicKey> {
    return this.createProgramAddressSync(seeds, programId);
  }

  /**
   * Find a valid program address
   *
   * Valid program addresses must fall off the ed25519 curve.  This function
   * iterates a nonce until it finds one that when combined with the seeds
   * results in a valid program address.
   */
  static findProgramAddressSync(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): [PublicKey, number] {
    let nonce = 255;
    let address;
    while (nonce != 0) {
      try {
        const seedsWithNonce = seeds.concat(Buffer.from([nonce]));
        address = this.createProgramAddressSync(seedsWithNonce, programId);
      } catch (err) {
        if (err instanceof TypeError) {
          throw err;
        }
        nonce--;
        continue;
      }
      return [address, nonce];
    }
    throw new Error(`Unable to find a viable program address nonce`);
  }

  /**
   * Async version of findProgramAddressSync
   * For backwards compatibility
   */
  static async findProgramAddress(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): Promise<[PublicKey, number]> {
    return this.findProgramAddressSync(seeds, programId);
  }

  /**
   * Check that a pubkey is on the ed25519 curve.
   */
  static isOnCurve(pubkeyData: PublicKeyInitData): boolean {
    const pubkey = new PublicKey(pubkeyData);
    return isOnCurve(pubkey.toBytes());
  }
}

SOLANA_SCHEMA.set(PublicKey, {
  kind: 'struct',
  fields: [['_buffer', 'u256']],
});
