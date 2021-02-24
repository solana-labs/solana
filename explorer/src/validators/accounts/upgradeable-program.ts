/* eslint-disable @typescript-eslint/no-redeclare */

import { StructType, pick, number, nullable, literal } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type ProgramAccountInfo = StructType<typeof ProgramAccountInfo>;
export const ProgramAccountInfo = pick({
  programData: Pubkey,
});

export type ProgramAccount = StructType<typeof ProgramDataAccount>;
export const ProgramAccount = pick({
  type: literal("program"),
  info: ProgramAccountInfo,
});

export type ProgramDataAccountInfo = StructType<typeof ProgramDataAccountInfo>;
export const ProgramDataAccountInfo = pick({
  authority: nullable(Pubkey),
  // don't care about data yet
  slot: number(),
});

export type ProgramDataAccount = StructType<typeof ProgramDataAccount>;
export const ProgramDataAccount = pick({
  type: literal("programData"),
  info: ProgramDataAccountInfo,
});
