// @flow

import BN from 'bn.js';
import bs58 from 'bs58';
import nacl from 'tweetnacl';
import {sha256} from 'crypto-hash';

//$FlowFixMe
let naclLowLevel = nacl.lowlevel;

/**
 * A public key
 */
export class PublicKey {
  _bn: BN;

  /**
   * Create a new PublicKey object
   */
  constructor(value: number | string | Buffer | Uint8Array | Array<number>) {
    if (typeof value === 'string') {
      // hexadecimal number
      if (value.startsWith('0x')) {
        this._bn = new BN(value.substring(2), 16);
      } else {
        // assume base 58 encoding by default
        const decoded = bs58.decode(value);
        if (decoded.length != 32) {
          throw new Error(`Invalid public key input`);
        }
        this._bn = new BN(decoded);
      }
    } else {
      this._bn = new BN(value);
    }

    if (this._bn.byteLength() > 32) {
      throw new Error(`Invalid public key input`);
    }
  }

  /**
   * Checks if the provided object is a PublicKey
   */
  static isPublicKey(o: Object): boolean {
    return o instanceof PublicKey;
  }

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
    return bs58.encode(this.toBuffer());
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
   * Returns a string representation of the public key
   */
  toString(): string {
    return this.toBase58();
  }

  /**
   * Derive a public key from another key, a seed, and a program ID.
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
    return new PublicKey('0x' + hash);
  }

  /**
   * Derive a program address from seed bytes and a program ID.
   */
  static async createProgramAddress(
    seeds: Array<Buffer | Uint8Array>,
    programId: PublicKey,
  ): Promise<PublicKey> {
    let buffer = Buffer.alloc(0);
    seeds.forEach(function (seed) {
      buffer = Buffer.concat([buffer, Buffer.from(seed)]);
    });
    buffer = Buffer.concat([
      buffer,
      programId.toBuffer(),
      Buffer.from('ProgramDerivedAddress'),
    ]);
    let hash = await sha256(new Uint8Array(buffer));
    let seed = new BN(hash, 16).toBuffer();

    seed[0] &= 248;
    seed[31] &= 127;
    seed[31] |= 64;

    var p = [
      naclLowLevel.gf(),
      naclLowLevel.gf(),
      naclLowLevel.gf(),
      naclLowLevel.gf(),
    ];
    naclLowLevel.scalarbase(p, seed);
    var pk = new Uint8Array(naclLowLevel.crypto_sign_PUBLICKEYBYTES);
    pack(pk, p);

    return new PublicKey(pk);
  }
}

function inv25519(o, i) {
  var c = naclLowLevel.gf();
  var a;
  for (a = 0; a < 16; a++) c[a] = i[a];
  for (a = 253; a >= 0; a--) {
    naclLowLevel.S(c, c);
    if (a !== 2 && a !== 4) naclLowLevel.M(c, c, i);
  }
  for (a = 0; a < 16; a++) o[a] = c[a];
}

function par25519(a) {
  var d = new Uint8Array(32);
  naclLowLevel.pack25519(d, a);
  return d[0] & 1;
}

function pack(r, p) {
  var tx = naclLowLevel.gf(),
    ty = naclLowLevel.gf(),
    zi = naclLowLevel.gf();
  inv25519(zi, p[2]);
  naclLowLevel.M(tx, p[0], zi);
  naclLowLevel.M(ty, p[1], zi);
  naclLowLevel.pack25519(r, ty);
  r[31] ^= par25519(tx) << 7;
}
