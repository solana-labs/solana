/* eslint-disable @typescript-eslint/no-redeclare */

import {
  array,
  nullable,
  number,
  optional,
  type,
  string,
  Infer,
} from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type InitializeInfo = Infer<typeof InitializeInfo>;
export const InitializeInfo = type({
  voteAccount: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
  node: PublicKeyFromString,
  authorizedVoter: PublicKeyFromString,
  authorizedWithdrawer: PublicKeyFromString,
  commission: number(),
});

export type AuthorizeInfo = Infer<typeof AuthorizeInfo>;
export const AuthorizeInfo = type({
  voteAccount: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
  authority: PublicKeyFromString,
  newAuthority: PublicKeyFromString,
  authorityType: number(),
});

export type VoteInfo = Infer<typeof VoteInfo>;
export const VoteInfo = type({
  clockSysvar: PublicKeyFromString,
  slotHashesSysvar: PublicKeyFromString,
  voteAccount: PublicKeyFromString,
  voteAuthority: PublicKeyFromString,
  vote: type({
    hash: string(),
    slots: array(number()),
    timestamp: optional(nullable(number())),
  }),
});

export type WithdrawInfo = Infer<typeof WithdrawInfo>;
export const WithdrawInfo = type({
  voteAccount: PublicKeyFromString,
  destination: PublicKeyFromString,
  withdrawAuthority: PublicKeyFromString,
  lamports: number(),
});

export type UpdateValidatorInfo = Infer<typeof UpdateValidatorInfo>;
export const UpdateValidatorInfo = type({
  voteAccount: PublicKeyFromString,
  newValidatorIdentity: PublicKeyFromString,
  withdrawAuthority: PublicKeyFromString,
});

export type UpdateCommissionInfo = Infer<typeof UpdateCommissionInfo>;
export const UpdateCommissionInfo = type({
  voteAccount: PublicKeyFromString,
  withdrawAuthority: PublicKeyFromString,
  commission: number(),
});

export type VoteSwitchInfo = Infer<typeof VoteSwitchInfo>;
export const VoteSwitchInfo = type({
  voteAccount: PublicKeyFromString,
  slotHashesSysvar: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
  voteAuthority: PublicKeyFromString,
  vote: type({
    hash: string(),
    slots: array(number()),
    timestamp: number(),
  }),
  hash: string(),
});
