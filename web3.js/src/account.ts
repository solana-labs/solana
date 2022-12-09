import {Buffer} from 'buffer';

import {generatePrivateKey, getPublicKey} from './utils/ed25519';
import {toBuffer} from './utils/to-buffer';
import {PublicKey} from './publickey';

/**
 * An account key pair (public and secret keys).
 *
 * @deprecated since v1.10.0, please use {@link Keypair} instead.
 */
export class Account {
  /** @internal */
  private _publicKey: Buffer;
  /** @internal */
  private _secretKey: Buffer;

  /**
   * Create a new Account object
   *
   * If the secretKey parameter is not provided a new key pair is randomly
   * created for the account
   *
   * @param secretKey Secret key for the account
   */
  constructor(secretKey?: Uint8Array | Array<number>) {
    if (secretKey) {
      const secretKeyBuffer = toBuffer(secretKey);
      if (secretKey.length !== 64) {
        throw new Error('bad secret key size');
      }
      this._publicKey = secretKeyBuffer.slice(32, 64);
      this._secretKey = secretKeyBuffer.slice(0, 32);
    } else {
      this._secretKey = toBuffer(generatePrivateKey());
      this._publicKey = toBuffer(getPublicKey(this._secretKey));
    }
  }

  /**
   * The public key for this account
   */
  get publicKey(): PublicKey {
    return new PublicKey(this._publicKey);
  }

  /**
   * The **unencrypted** secret key for this account. The first 32 bytes
   * is the private scalar and the last 32 bytes is the public key.
   * Read more: https://blog.mozilla.org/warner/2011/11/29/ed25519-keys/
   */
  get secretKey(): Buffer {
    return Buffer.concat([this._secretKey, this._publicKey], 64);
  }
}
