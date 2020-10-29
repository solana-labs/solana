/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type InitializeInfo = StructType<typeof InitializeInfo>;
export const InitializeInfo = pick({
  stakeAccount: Pubkey,
  authorized: pick({
    staker: Pubkey,
    withdrawer: Pubkey,
  }),
  lockup: pick({
    unixTimestamp: number(),
    epoch: number(),
    custodian: Pubkey,
  }),
});

export type DelegateInfo = StructType<typeof DelegateInfo>;
export const DelegateInfo = pick({
  stakeAccount: Pubkey,
  voteAccount: Pubkey,
  stakeAuthority: Pubkey,
});

export type AuthorizeInfo = StructType<typeof AuthorizeInfo>;
export const AuthorizeInfo = pick({
  authorityType: string(),
  stakeAccount: Pubkey,
  authority: Pubkey,
  newAuthority: Pubkey,
});

export type SplitInfo = StructType<typeof SplitInfo>;
export const SplitInfo = pick({
  stakeAccount: Pubkey,
  stakeAuthority: Pubkey,
  newSplitAccount: Pubkey,
  lamports: number(),
});

export type WithdrawInfo = StructType<typeof WithdrawInfo>;
export const WithdrawInfo = pick({
  stakeAccount: Pubkey,
  withdrawAuthority: Pubkey,
  destination: Pubkey,
  lamports: number(),
});

export type DeactivateInfo = StructType<typeof DeactivateInfo>;
export const DeactivateInfo = pick({
  stakeAccount: Pubkey,
  stakeAuthority: Pubkey,
});

export type StakeInstructionType = StructType<typeof StakeInstructionType>;
export const StakeInstructionType = enums([
  "initialize",
  "delegate",
  "authorize",
  "split",
  "withdraw",
  "deactivate",
]);
