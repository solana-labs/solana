import * as BufferLayout from '@solana/buffer-layout';
import {Buffer} from 'buffer';

import type {Blockhash} from './blockhash';
import * as Layout from './layout';
import {PublicKey} from './publickey';
import type {FeeCalculator} from './fee-calculator';
import {FeeCalculatorLayout} from './fee-calculator';
import {toBuffer} from './util/to-buffer';

/**
 * See https://github.com/solana-labs/solana/blob/0ea2843ec9cdc517572b8e62c959f41b55cf4453/sdk/src/nonce_state.rs#L29-L32
 *
 * @internal
 */
const NonceAccountLayout = BufferLayout.struct<
  Readonly<{
    authorizedPubkey: Uint8Array;
    feeCalculator: Readonly<{
      lamportsPerSignature: number;
    }>;
    nonce: Uint8Array;
    state: number;
    version: number;
  }>
>([
  BufferLayout.u32('version'),
  BufferLayout.u32('state'),
  Layout.publicKey('authorizedPubkey'),
  Layout.publicKey('nonce'),
  BufferLayout.struct<Readonly<{lamportsPerSignature: number}>>(
    [FeeCalculatorLayout],
    'feeCalculator',
  ),
]);

export const NONCE_ACCOUNT_LENGTH = NonceAccountLayout.span;

type NonceAccountArgs = {
  authorizedPubkey: PublicKey;
  nonce: Blockhash;
  feeCalculator: FeeCalculator;
};

/**
 * NonceAccount class
 */
export class NonceAccount {
  authorizedPubkey: PublicKey;
  nonce: Blockhash;
  feeCalculator: FeeCalculator;

  /**
   * @internal
   */
  constructor(args: NonceAccountArgs) {
    this.authorizedPubkey = args.authorizedPubkey;
    this.nonce = args.nonce;
    this.feeCalculator = args.feeCalculator;
  }

  /**
   * Deserialize NonceAccount from the account data.
   *
   * @param buffer account data
   * @return NonceAccount
   */
  static fromAccountData(
    buffer: Buffer | Uint8Array | Array<number>,
  ): NonceAccount {
    const nonceAccount = NonceAccountLayout.decode(toBuffer(buffer), 0);
    return new NonceAccount({
      authorizedPubkey: new PublicKey(nonceAccount.authorizedPubkey),
      nonce: new PublicKey(nonceAccount.nonce).toString(),
      feeCalculator: nonceAccount.feeCalculator,
    });
  }
}
