import * as BufferLayout from '@solana/buffer-layout';
import type {Buffer} from 'buffer';

import * as Layout from './layout';
import {PublicKey} from './publickey';
import {toBuffer} from './util/to-buffer';

export const VOTE_PROGRAM_ID = new PublicKey(
  'Vote111111111111111111111111111111111111111',
);

export type Lockout = {
  slot: number;
  confirmationCount: number;
};

/**
 * History of how many credits earned by the end of each epoch
 */
export type EpochCredits = {
  epoch: number;
  credits: number;
  prevCredits: number;
};

/**
 * See https://github.com/solana-labs/solana/blob/8a12ed029cfa38d4a45400916c2463fb82bbec8c/programs/vote_api/src/vote_state.rs#L68-L88
 *
 * @internal
 */
const VoteAccountLayout = BufferLayout.struct([
  Layout.publicKey('nodePubkey'),
  Layout.publicKey('authorizedVoterPubkey'),
  Layout.publicKey('authorizedWithdrawerPubkey'),
  BufferLayout.u8('commission'),
  BufferLayout.nu64(), // votes.length
  BufferLayout.seq(
    BufferLayout.struct([
      BufferLayout.nu64('slot'),
      BufferLayout.u32('confirmationCount'),
    ]),
    BufferLayout.offset(BufferLayout.u32(), -8),
    'votes',
  ),
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

type VoteAccountArgs = {
  nodePubkey: PublicKey;
  authorizedVoterPubkey: PublicKey;
  authorizedWithdrawerPubkey: PublicKey;
  commission: number;
  votes: Array<Lockout>;
  rootSlot: number | null;
  epoch: number;
  credits: number;
  lastEpochCredits: number;
  epochCredits: Array<EpochCredits>;
};

/**
 * VoteAccount class
 */
export class VoteAccount {
  nodePubkey: PublicKey;
  authorizedVoterPubkey: PublicKey;
  authorizedWithdrawerPubkey: PublicKey;
  commission: number;
  votes: Array<Lockout>;
  rootSlot: number | null;
  epoch: number;
  credits: number;
  lastEpochCredits: number;
  epochCredits: Array<EpochCredits>;

  /**
   * @internal
   */
  constructor(args: VoteAccountArgs) {
    this.nodePubkey = args.nodePubkey;
    this.authorizedVoterPubkey = args.authorizedVoterPubkey;
    this.authorizedWithdrawerPubkey = args.authorizedWithdrawerPubkey;
    this.commission = args.commission;
    this.votes = args.votes;
    this.rootSlot = args.rootSlot;
    this.epoch = args.epoch;
    this.credits = args.credits;
    this.lastEpochCredits = args.lastEpochCredits;
    this.epochCredits = args.epochCredits;
  }

  /**
   * Deserialize VoteAccount from the account data.
   *
   * @param buffer account data
   * @return VoteAccount
   */
  static fromAccountData(
    buffer: Buffer | Uint8Array | Array<number>,
  ): VoteAccount {
    const va = VoteAccountLayout.decode(toBuffer(buffer), 0);

    let rootSlot: number | null = va.rootSlot;
    if (!va.rootSlotValid) {
      rootSlot = null;
    }

    return new VoteAccount({
      nodePubkey: new PublicKey(va.nodePubkey),
      authorizedVoterPubkey: new PublicKey(va.authorizedVoterPubkey),
      authorizedWithdrawerPubkey: new PublicKey(va.authorizedWithdrawerPubkey),
      commission: va.commission,
      votes: va.votes,
      rootSlot,
      epoch: va.epoch,
      credits: va.credits,
      lastEpochCredits: va.lastEpochCredits,
      epochCredits: va.epochCredits,
    });
  }
}
