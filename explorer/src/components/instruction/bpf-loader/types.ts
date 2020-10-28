/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

const Write = pick({
  account: Pubkey,
  bytes: string(),
  offset: number(),
});

const Finalize = pick({
  account: Pubkey,
});

export type BpfLoaderInstructionType = StructType<
  typeof BpfLoaderInstructionType
>;
export const BpfLoaderInstructionType = enums(["write", "finalize"]);

export const IX_STRUCTS: { [id: string]: any } = {
  write: Write,
  finalize: Finalize,
};
