// @flow
import { ed25519 } from '@solana/wasm';

import {toBuffer} from './util/to-buffer';
import {PublicKey} from './publickey';

/**
 * An account key pair (public and secret keys).
 */
export class Account {
  _keypair;

  /**
   * Create a new Account object
   *
   * If the secretKey parameter is not provided a new key pair is randomly
   * created for the account
   *
   * @param secretKey Secret key for the account
   */
  constructor(secretKey?: Buffer | Uint8Array | Array<number>) {
    if (secretKey) {
      this._keypair = ed25519.keypair.fromSecretKey(toBuffer(secretKey));
    } else {
      this._keypair = ed25519.keypair.generate();
    }
  }

  /**
   * The public key for this account
   */
  get publicKey(): PublicKey {
    return new PublicKey(this._keypair.publicKey);
  }

  /**
   * The **unencrypted** secret key for this account
   */
  get secretKey(): Buffer {
    return Uint8Array.from(this._keypair.secretKey);
  }
}
