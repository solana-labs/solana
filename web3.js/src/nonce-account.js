// @flow
import * as BufferLayout from 'buffer-layout';

import type {Blockhash} from './blockhash';
import * as Layout from './layout';
import {PublicKey} from './publickey';

/**
 * See https://github.com/solana-labs/solana/blob/0ea2843ec9cdc517572b8e62c959f41b55cf4453/sdk/src/nonce_state.rs#L29-L32
 *
 * @private
 */
const NonceAccountLayout = BufferLayout.struct([
  BufferLayout.u32('state'),
  Layout.publicKey('authorizedPubkey'),
  Layout.publicKey('nonce'),
]);

/**
 * NonceAccount class
 */
export class NonceAccount {
  authorizedPubkey: PublicKey;
  nonce: Blockhash;

  /**
   * Deserialize NonceAccount from the account data.
   *
   * @param buffer account data
   * @return NonceAccount
   */
  static fromAccountData(buffer: Buffer): NonceAccount {
    const nonceAccount = NonceAccountLayout.decode(buffer, 0);
    nonceAccount.authorizedPubkey = new PublicKey(
      nonceAccount.authorizedPubkey,
    );
    nonceAccount.nonce = new PublicKey(nonceAccount.nonce).toString();
    return nonceAccount;
  }
}
