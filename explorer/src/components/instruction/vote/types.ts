/* eslint-disable @typescript-eslint/no-redeclare */

import {
  array,
  nullable,
  number,
  optional,
  pick,
  string,
  StructType,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

export type InitializeInfo = StructType<typeof InitializeInfo>;
export const InitializeInfo = pick({
  voteAccount: Pubkey,
  rentSysvar: Pubkey,
  clockSysvar: Pubkey,
  node: Pubkey,
  authorizedVoter: Pubkey,
  authorizedWithdrawer: Pubkey,
  commission: number(),
});

export type AuthorizeInfo = StructType<typeof AuthorizeInfo>;
export const AuthorizeInfo = pick({
  voteAccount: Pubkey,
  clockSysvar: Pubkey,
  authority: Pubkey,
  newAuthority: Pubkey,
  authorityType: number(),
});

export type VoteInfo = StructType<typeof VoteInfo>;
export const VoteInfo = pick({
  clockSysvar: Pubkey,
  slotHashesSysvar: Pubkey,
  voteAccount: Pubkey,
  voteAuthority: Pubkey,
  vote: pick({
    hash: string(),
    slots: array(number()),
    timestamp: optional(nullable(number())),
  }),
});

export type WithdrawInfo = StructType<typeof WithdrawInfo>;
export const WithdrawInfo = pick({
  voteAccount: Pubkey,
  destination: Pubkey,
  withdrawAuthority: Pubkey,
  lamports: number(),
});

export type UpdateValidatorInfo = StructType<typeof UpdateValidatorInfo>;
export const UpdateValidatorInfo = pick({
  voteAccount: Pubkey,
  newValidatorIdentity: Pubkey,
  withdrawAuthority: Pubkey,
});

export type UpdateCommissionInfo = StructType<typeof UpdateCommissionInfo>;
export const UpdateCommissionInfo = pick({
  voteAccount: Pubkey,
  withdrawAuthority: Pubkey,
  commission: number(),
});

export type VoteSwitchInfo = StructType<typeof VoteSwitchInfo>;
export const VoteSwitchInfo = pick({
  voteAccount: Pubkey,
  slotHashesSysvar: Pubkey,
  clockSysvar: Pubkey,
  voteAuthority: Pubkey,
  vote: pick({
    hash: string(),
    slots: array(number()),
    timestamp: number(),
  }),
  hash: string(),
});
