/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

const Initialize = pick({
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

const Delegate = pick({
  stakeAccount: Pubkey,
  voteAccount: Pubkey,
  stakeAuthority: Pubkey,
});

const Authorize = pick({
  authorityType: string(),
  stakeAccount: Pubkey,
  authority: Pubkey,
  newAuthority: Pubkey,
});

const Split = pick({
  stakeAccount: Pubkey,
  stakeAuthority: Pubkey,
  newSplitAccount: Pubkey,
  lamports: number(),
});

const Withdraw = pick({
  stakeAccount: Pubkey,
  withdrawAuthority: Pubkey,
  destination: Pubkey,
  lamports: number(),
});

const Deactivate = pick({
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

export const IX_STRUCTS: { [id: string]: any } = {
  initialize: Initialize,
  delegate: Delegate,
  authorize: Authorize,
  split: Split,
  withdraw: Withdraw,
  deactivate: Deactivate,
};
