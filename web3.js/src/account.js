// @flow
import nacl from 'tweetnacl';
import type {KeyPair} from 'tweetnacl';

import {toBuffer} from './util/to-buffer';
import {PublicKey} from './publickey';

/**
 * An account key pair (public and secret keys).
 */
export class Account {
  _keypair: KeyPair;

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
      this._keypair = nacl.sign.keyPair.fromSecretKey(toBuffer(secretKey));
    } else {
      this._keypair = nacl.sign.keyPair();
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
    return this._keypair.secretKey;
  }
}
