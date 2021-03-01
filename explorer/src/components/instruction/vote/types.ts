/* eslint-disable @typescript-eslint/no-redeclare */

import { array, number, optional, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type VoteInfo = StructType<typeof VoteInfo>;
export const VoteInfo = pick({
  clockSysvar: Pubkey,
  slotHashesSysvar: Pubkey,
  voteAccount: Pubkey,
  voteAuthority: Pubkey,
  vote: pick({
    hash: string(),
    slots: array(number()),
    timestamp: optional(number()),
  }),
});
