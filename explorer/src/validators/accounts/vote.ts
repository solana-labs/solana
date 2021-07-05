/* eslint-disable @typescript-eslint/no-redeclare */

import {
  Infer,
  enums,
  number,
  array,
  type,
  nullable,
  string,
} from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type VoteAccountType = Infer<typeof VoteAccountType>;
export const VoteAccountType = enums(["vote"]);

export type AuthorizedVoter = Infer<typeof AuthorizedVoter>;
export const AuthorizedVoter = type({
  authorizedVoter: PublicKeyFromString,
  epoch: number(),
});

export type PriorVoter = Infer<typeof PriorVoter>;
export const PriorVoter = type({
  authorizedPubkey: PublicKeyFromString,
  epochOfLastAuthorizedSwitch: number(),
  targetEpoch: number(),
});

export type EpochCredits = Infer<typeof EpochCredits>;
export const EpochCredits = type({
  epoch: number(),
  credits: string(),
  previousCredits: string(),
});

export type Vote = Infer<typeof Vote>;
export const Vote = type({
  slot: number(),
  confirmationCount: number(),
});

export type VoteAccountInfo = Infer<typeof VoteAccountInfo>;
export const VoteAccountInfo = type({
  authorizedVoters: array(AuthorizedVoter),
  authorizedWithdrawer: PublicKeyFromString,
  commission: number(),
  epochCredits: array(EpochCredits),
  lastTimestamp: type({
    slot: number(),
    timestamp: number(),
  }),
  nodePubkey: PublicKeyFromString,
  priorVoters: array(PriorVoter),
  rootSlot: nullable(number()),
  votes: array(Vote),
});

export type VoteAccount = Infer<typeof VoteAccount>;
export const VoteAccount = type({
  type: VoteAccountType,
  info: VoteAccountInfo,
});
