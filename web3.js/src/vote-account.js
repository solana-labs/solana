// @flow
import * as BufferLayout from 'buffer-layout';

import * as Layout from './layout';
import {PublicKey} from './publickey';

export const VOTE_ACCOUNT_KEY = new PublicKey(
  'Vote111111111111111111111111111111111111111',
);

export type Lockout = {|
  slot: number,
  confirmationCount: number,
|};

/**
 * History of how many credits earned by the end of each epoch
 */
export type EpochCredits = {|
  epoch: number,
  credits: number,
  prevCredits: number,
|};

/**
 * See https://github.com/solana-labs/solana/blob/8a12ed029cfa38d4a45400916c2463fb82bbec8c/programs/vote_api/src/vote_state.rs#L68-L88
 *
 * @private
 */
const VoteAccountLayout = BufferLayout.struct([
  BufferLayout.nu64(), // votes.length
  BufferLayout.seq(
    BufferLayout.struct([
      BufferLayout.nu64('slot'),
      BufferLayout.u32('confirmationCount'),
    ]),
    BufferLayout.offset(BufferLayout.u32(), -8),
    'votes',
  ),
  Layout.publicKey('nodePubkey'),
  Layout.publicKey('authorizedVoterPubkey'),
  BufferLayout.u8('commission'),
  BufferLayout.u8('rootSlotValid'),
  BufferLayout.nu64('rootSlot'),
  BufferLayout.nu64('epoch'),
  BufferLayout.nu64('credits'),
  BufferLayout.nu64('lastEpochCredits'),
  BufferLayout.nu64(), // epochCredits.length
  BufferLayout.seq(
    BufferLayout.struct([
      BufferLayout.nu64('epoch'),
      BufferLayout.nu64('credits'),
      BufferLayout.nu64('prevCredits'),
    ]),
    BufferLayout.offset(BufferLayout.u32(), -8),
    'epochCredits',
  ),
]);

/**
 * VoteAccount class
 */
export class VoteAccount {
  votes: Array<Lockout>;
  nodePubkey: PublicKey;
  authorizedVoterPubkey: PublicKey;
  commission: number;
  rootSlot: number | null;
  epoch: number;
  credits: number;
  lastEpochCredits: number;
  epochCredits: Array<EpochCredits>;

  /**
   * Deserialize VoteAccount from the account data.
   *
   * @param buffer account data
   * @return VoteAccount
   */
  static fromAccountData(buffer: Buffer): VoteAccount {
    const va = VoteAccountLayout.decode(buffer, 0);
    va.nodePubkey = new PublicKey(va.nodePubkey);
    va.authorizedVoterPubkey = new PublicKey(va.authorizedVoterPubkey);
    if (!va.rootSlotValid) {
      va.rootSlot = null;
    }
    return va;
  }
}
