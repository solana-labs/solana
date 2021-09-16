import BN from 'bn.js';
import bs58 from 'bs58';
import {Buffer} from 'buffer';
import nacl from 'tweetnacl';
import {sha256} from 'crypto-hash';

import {Struct, SOLANA_SCHEMA} from './util/borsh-schema';
import {toBuffer} from './util/to-buffer';

/**
 * Maximum length of derived pubkey seed
 */
export const MAX_SEED_LENGTH = 32;

/**
 * Value to be converted into public key
 */
export type PublicKeyInitData =
  | number
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
  _bn: BN;
};

function isPublicKeyData(value: PublicKeyInitData): value is PublicKeyData {
  return (value as PublicKeyData)._bn !== undefined;
}

/**
 * A public key
 */
export class PublicKey extends Struct {
  /** @internal */
  _bn: BN;

  /**
   * Create a new PublicKey object
   * @param value ed25519 public key as buffer or base-58 encoded string
   */
  constructor(value: PublicKeyInitData) {
    super({});
    if (isPublicKeyData(value)) {
      this._bn = value._bn;
    } else {
      if (typeof value === 'string') {
        // assume base 58 encoding by default
        const decoded = bs58.decode(value);
        if (decoded.length != 32) {
          throw new Error(`Invalid public key input`);
        }
        this._bn = new BN(decoded);
      } else {
        this._bn = new BN(value);
      }

      if (this._bn.byteLength() > 32) {
        throw new Error(`Invalid public key input`);
      }
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
    return this._bn.eq(publicKey._bn);
  }

  /**
   * Return the base-58 representation of the public key
   */
  toBase58(): string {
    return bs58.encode(this.toBytes());
  }

  /**
   * Return the byte array representation of the public key
   */
  toBytes(): Uint8Array {
    return this.toBuffer();
  }

  /**
   * Return the Buffer representation of the public key
   */
  toBuffer(): Buffer {
    const b = this._bn.toArrayLike(Buffer);
    if (b.length === 32) {
      return b;
    }

    const zeroPad = Buffer.alloc(32);
    b.copy(zeroPad, 32 - b.length);
    return zeroPad;
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
    const hash = await sha256(new Uint8Array(buffer));
    return new PublicKey(Buffer.from(hash, 'hex'));
  }

  /**
   * Derive a program address from seeds and a program ID.
   */
  static async createProgramAddress(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): Promise<PublicKey> {
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
    let hash = await sha256(new Uint8Array(buffer));
    let publicKeyBytes = new BN(hash, 16).toArray(undefined, 32);
    if (is_on_curve(publicKeyBytes)) {
      throw new Error(`Invalid seeds, address must fall off the curve`);
    }
    return new PublicKey(publicKeyBytes);
  }

  /**
   * Find a valid program address
   *
   * Valid program addresses must fall off the ed25519 curve.  This function
   * iterates a nonce until it finds one that when combined with the seeds
   * results in a valid program address.
   */
  static async findProgramAddress(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): Promise<[PublicKey, number]> {
    let nonce = 255;
    let address;
    while (nonce != 0) {
      try {
        const seedsWithNonce = seeds.concat(Buffer.from([nonce]));
        address = await this.createProgramAddress(seedsWithNonce, programId);
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
   * Check that a pubkey is on the ed25519 curve.
   */
  static isOnCurve(pubkey: Uint8Array): boolean {
    return is_on_curve(pubkey) == 1;
  }
}

SOLANA_SCHEMA.set(PublicKey, {
  kind: 'struct',
  fields: [['_bn', 'u256']],
});

// @ts-ignore
let naclLowLevel = nacl.lowlevel;

// Check that a pubkey is on the curve.
// This function and its dependents were sourced from:
// https://github.com/dchest/tweetnacl-js/blob/f1ec050ceae0861f34280e62498b1d3ed9c350c6/nacl.js#L792
function is_on_curve(p: any) {
  var r = [
    naclLowLevel.gf(),
    naclLowLevel.gf(),
    naclLowLevel.gf(),
    naclLowLevel.gf(),
  ];

  var t = naclLowLevel.gf(),
    chk = naclLowLevel.gf(),
    num = naclLowLevel.gf(),
    den = naclLowLevel.gf(),
    den2 = naclLowLevel.gf(),
    den4 = naclLowLevel.gf(),
    den6 = naclLowLevel.gf();

  naclLowLevel.set25519(r[2], gf1);
  naclLowLevel.unpack25519(r[1], p);
  naclLowLevel.S(num, r[1]);
  naclLowLevel.M(den, num, naclLowLevel.D);
  naclLowLevel.Z(num, num, r[2]);
  naclLowLevel.A(den, r[2], den);

  naclLowLevel.S(den2, den);
  naclLowLevel.S(den4, den2);
  naclLowLevel.M(den6, den4, den2);
  naclLowLevel.M(t, den6, num);
  naclLowLevel.M(t, t, den);

  naclLowLevel.pow2523(t, t);
  naclLowLevel.M(t, t, num);
  naclLowLevel.M(t, t, den);
  naclLowLevel.M(t, t, den);
  naclLowLevel.M(r[0], t, den);

  naclLowLevel.S(chk, r[0]);
  naclLowLevel.M(chk, chk, den);
  if (neq25519(chk, num)) naclLowLevel.M(r[0], r[0], I);

  naclLowLevel.S(chk, r[0]);
  naclLowLevel.M(chk, chk, den);
  if (neq25519(chk, num)) return 0;
  return 1;
}
let gf1 = naclLowLevel.gf([1]);
let I = naclLowLevel.gf([
  0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7,
  0x3dfb, 0x0099, 0x2b4d, 0xdf0b, 0x4fc1, 0x2480, 0x2b83,
]);
function neq25519(a: any, b: any) {
  var c = new Uint8Array(32),
    d = new Uint8Array(32);
  naclLowLevel.pack25519(c, a);
  naclLowLevel.pack25519(d, b);
  return naclLowLevel.crypto_verify_32(c, 0, d, 0);
}
