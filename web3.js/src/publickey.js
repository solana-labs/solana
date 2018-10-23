// @flow

import BN from 'bn.js';
import bs58 from 'bs58';

/**
 * A public key
 */
export class PublicKey {
  _bn: BN;

  /**
   * Create a new PublicKey object
   */
  constructor(number: number | string | Buffer | Array<number>) {

    for (;;) {
      if (typeof number === 'string') {
        // base 58 encoded?
        if (/^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(number)) {
          this._bn = new BN(bs58.decode(number));
          break;
        }

        // hexadecimal number
        if (number.startsWith('0x')) {
          this._bn = new BN(number.substring(2), 16);
          break;
        }
      }

      this._bn = new BN(number);
      break;
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
}

