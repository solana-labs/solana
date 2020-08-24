import { StructType, enums, pick, number, array, object, nullable } from "superstruct"
import { Pubkey } from "validators/pubkey";

export type VoteAccountType = StructType<typeof VoteAccountType>;
export const VoteAccountType = enums([
  "vote"
]);

export type Voter = StructType<typeof Voter>;
export const Voter = pick({
  authorizedVoter: Pubkey,
  epoch: number()
});

export type VoteAccountInfo = StructType<typeof VoteAccountInfo>;
export const VoteAccountInfo = pick({
  authorizedVoters: array(Voter),
  authorizedWithdrawer: Pubkey,
  commission: number(),
  epochCredits: array(),
  lastTimestamp: object({
    slot: number(),
    timestamp: number()
  }),
  nodePubkey: Pubkey,
  priorVoters: array(Voter),
  rootSlot: nullable(number()),
  votes: array()
});

export type VoteAccount = StructType<typeof VoteAccount>;
export const VoteAccount = pick({
  type: VoteAccountType,
  info: VoteAccountInfo
});
