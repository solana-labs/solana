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

export type AuthorizedVoter = {
  epoch: number;
  authorizedVoter: PublicKey;
};

export type PriorVoter = {
  authorizedPubkey: PublicKey;
  epochOfLastAuthorizedSwitch: number;
  targetEpoch: number;
};

export type BlockTimestamp = {
  slot: number;
  timetamp: number;
};

/**
 * See https://github.com/solana-labs/solana/blob/8a12ed029cfa38d4a45400916c2463fb82bbec8c/programs/vote_api/src/vote_state.rs#L68-L88
 *
 * @internal
 */
const VoteAccountLayout = BufferLayout.struct([
  Layout.publicKey('nodePubkey'),
  Layout.publicKey('authorizedWithdrawer'),
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
  BufferLayout.nu64(), // authorizedVoters.length
  BufferLayout.seq(
    BufferLayout.struct([
      BufferLayout.nu64('epoch'),
      Layout.publicKey('authorizedVoter'),
    ]),
    BufferLayout.offset(BufferLayout.u32(), -8),
    'authorizedVoters',
  ),
  BufferLayout.struct(
    [
      BufferLayout.seq(
        BufferLayout.struct([
          Layout.publicKey('authorizedPubkey'),
          BufferLayout.nu64('epochOfLastAuthorizedSwitch'),
          BufferLayout.nu64('targetEpoch'),
        ]),
        32,
        'buf',
      ),
      BufferLayout.nu64('idx'),
      BufferLayout.u8('isEmpty'),
    ],
    'priorVoters',
  ),
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
  BufferLayout.struct(
    [BufferLayout.nu64('slot'), BufferLayout.nu64('timestamp')],
    'lastTimestamp',
  ),
]);

type VoteAccountArgs = {
  nodePubkey: PublicKey;
  authorizedWithdrawer: PublicKey;
  commission: number;
  rootSlot: number | null;
  votes: Lockout[];
  authorizedVoters: AuthorizedVoter[];
  priorVoters: PriorVoter[];
  epochCredits: EpochCredits[];
  lastTimestamp: BlockTimestamp;
};

/**
 * VoteAccount class
 */
export class VoteAccount {
  nodePubkey: PublicKey;
  authorizedWithdrawer: PublicKey;
  commission: number;
  rootSlot: number | null;
  votes: Lockout[];
  authorizedVoters: AuthorizedVoter[];
  priorVoters: PriorVoter[];
  epochCredits: EpochCredits[];
  lastTimestamp: BlockTimestamp;

  /**
   * @internal
   */
  constructor(args: VoteAccountArgs) {
    this.nodePubkey = args.nodePubkey;
    this.authorizedWithdrawer = args.authorizedWithdrawer;
    this.commission = args.commission;
    this.rootSlot = args.rootSlot;
    this.votes = args.votes;
    this.authorizedVoters = args.authorizedVoters;
    this.priorVoters = args.priorVoters;
    this.epochCredits = args.epochCredits;
    this.lastTimestamp = args.lastTimestamp;
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
    const versionOffset = 4;
    const va = VoteAccountLayout.decode(toBuffer(buffer), versionOffset);

    let rootSlot: number | null = va.rootSlot;
    if (!va.rootSlotValid) {
      rootSlot = null;
    }

    return new VoteAccount({
      nodePubkey: new PublicKey(va.nodePubkey),
      authorizedWithdrawer: new PublicKey(va.authorizedWithdrawer),
      commission: va.commission,
      votes: va.votes,
      rootSlot,
      authorizedVoters: va.authorizedVoters.map(parseAuthorizedVoter),
      priorVoters: getPriorVoters(va.priorVoters),
      epochCredits: va.epochCredits,
      lastTimestamp: va.lastTimestamp,
    });
  }
}

function parseAuthorizedVoter({epoch, authorizedVoter}: AuthorizedVoter) {
  return {
    epoch,
    authorizedVoter: new PublicKey(authorizedVoter),
  };
}

function parsePriorVoters({
  authorizedPubkey,
  epochOfLastAuthorizedSwitch,
  targetEpoch,
}: PriorVoter) {
  return {
    authorizedPubkey: new PublicKey(authorizedPubkey),
    epochOfLastAuthorizedSwitch,
    targetEpoch,
  };
}

function getPriorVoters({
  buf,
  idx,
  isEmpty,
}: {
  buf: PriorVoter[];
  idx: number;
  isEmpty: boolean;
}): PriorVoter[] {
  if (isEmpty) {
    return [];
  }

  return [...buf.slice(idx + 1).map(parsePriorVoters), ...buf.slice(0, idx)];
}
