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
  constructor(number: string | Buffer | Array<number>) {
    let radix = 10;

    if (typeof number === 'string' && number.startsWith('0x')) {
      this._bn = new BN(number.substring(2), 16);
    } else {
      this._bn = new BN(number, radix);
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
   * Return the base-58 representation of the public key
   */
  toBuffer(): Buffer {
    const b = this._bn.toBuffer();
    if (b.length === 32) {
      return b;
    }

    const zeroPad = new Buffer(32);
    b.copy(zeroPad);
    return zeroPad;
  }

  /**
   * Returns a string representation of the public key
   */
  toString(): string {
    return this.toBase58();
  }
}

