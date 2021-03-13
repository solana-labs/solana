/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, type, string, Infer } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type InitializeInfo = Infer<typeof InitializeInfo>;
export const InitializeInfo = type({
  stakeAccount: PublicKeyFromString,
  authorized: type({
    staker: PublicKeyFromString,
    withdrawer: PublicKeyFromString,
  }),
  lockup: type({
    unixTimestamp: number(),
    epoch: number(),
    custodian: PublicKeyFromString,
  }),
});

export type DelegateInfo = Infer<typeof DelegateInfo>;
export const DelegateInfo = type({
  stakeAccount: PublicKeyFromString,
  voteAccount: PublicKeyFromString,
  stakeAuthority: PublicKeyFromString,
});

export type AuthorizeInfo = Infer<typeof AuthorizeInfo>;
export const AuthorizeInfo = type({
  authorityType: string(),
  stakeAccount: PublicKeyFromString,
  authority: PublicKeyFromString,
  newAuthority: PublicKeyFromString,
});

export type SplitInfo = Infer<typeof SplitInfo>;
export const SplitInfo = type({
  stakeAccount: PublicKeyFromString,
  stakeAuthority: PublicKeyFromString,
  newSplitAccount: PublicKeyFromString,
  lamports: number(),
});

export type WithdrawInfo = Infer<typeof WithdrawInfo>;
export const WithdrawInfo = type({
  stakeAccount: PublicKeyFromString,
  withdrawAuthority: PublicKeyFromString,
  destination: PublicKeyFromString,
  lamports: number(),
});

export type DeactivateInfo = Infer<typeof DeactivateInfo>;
export const DeactivateInfo = type({
  stakeAccount: PublicKeyFromString,
  stakeAuthority: PublicKeyFromString,
});

export type MergeInfo = Infer<typeof MergeInfo>;
export const MergeInfo = type({
  source: PublicKeyFromString,
  destination: PublicKeyFromString,
  stakeAuthority: PublicKeyFromString,
  stakeHistorySysvar: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
});

export type StakeInstructionType = Infer<typeof StakeInstructionType>;
export const StakeInstructionType = enums([
  "initialize",
  "delegate",
  "authorize",
  "split",
  "withdraw",
  "deactivate",
  "merge",
]);
