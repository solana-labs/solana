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
export const AuthorizedVoter = pick({
  authorizedVoter: Pubkey,
  epoch: number(),
});

export type PriorVoter = StructType<typeof PriorVoter>;
export const PriorVoter = pick({
  authorizedPubkey: Pubkey,
  epochOfLastAuthorizedSwitch: number(),
  targetEpoch: number(),
});

export type EpochCredits = StructType<typeof EpochCredits>;
export const EpochCredits = pick({
  epoch: number(),
  credits: string(),
  previousCredits: string(),
});

export type Vote = StructType<typeof Vote>;
export const Vote = object({
  slot: number(),
  confirmationCount: number(),
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
  priorVoters: array(PriorVoter),
  rootSlot: nullable(number()),
  votes: array(Vote),
});

export type VoteAccount = StructType<typeof VoteAccount>;
export const VoteAccount = pick({
  type: VoteAccountType,
  info: VoteAccountInfo,
});
