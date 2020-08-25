import {
  StructType,
  enums,
  pick,
  number,
  array,
  object,
  nullable,
  string,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

export type VoteAccountType = StructType<typeof VoteAccountType>;
export const VoteAccountType = enums(["vote"]);

export type AuthorizedVoter = StructType<typeof AuthorizedVoter>;
export const AuthorizedVoter = object({
  authorizedVoter: Pubkey,
  epoch: number(),
});

export type PriorVoter = StructType<typeof PriorVoter>;
export const PriorVoter = object({
  authorizedPubkey: Pubkey,
  epochOfLastAuthroizedSwitch: number(),
  targetEpoch: number(),
});

export type EpochCredits = StructType<typeof EpochCredits>;
export const EpochCredits = object({
  epoch: number(),
  credits: string(),
  previous_credits: string(),
});

export type Vote = StructType<typeof Vote>;
export const Vote = object({
  slot: number(),
  confirmation_count: number(),
});

export type VoteAccountInfo = StructType<typeof VoteAccountInfo>;
export const VoteAccountInfo = pick({
  authorizedVoters: array(AuthorizedVoter),
  authorizedWithdrawer: Pubkey,
  commission: number(),
  epochCredits: array(EpochCredits),
  lastTimestamp: object({
    slot: number(),
    timestamp: number(),
  }),
  nodePubkey: Pubkey,
  priorVoters: array(AuthorizedVoter),
  rootSlot: nullable(number()),
  votes: array(Vote),
});

export type VoteAccount = StructType<typeof VoteAccount>;
export const VoteAccount = pick({
  type: VoteAccountType,
  info: VoteAccountInfo,
});
