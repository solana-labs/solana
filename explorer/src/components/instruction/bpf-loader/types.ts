/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type WriteInfo = StructType<typeof WriteInfo>;
export const WriteInfo = pick({
  account: Pubkey,
  bytes: string(),
  offset: number(),
});

export type FinalizeInfo = StructType<typeof FinalizeInfo>;
export const FinalizeInfo = pick({
  account: Pubkey,
});

export type BpfLoaderInstructionType = StructType<
  typeof BpfLoaderInstructionType
>;
export const BpfLoaderInstructionType = enums(["write", "finalize"]);
