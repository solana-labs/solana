/* eslint-disable @typescript-eslint/no-redeclare */

import { type, number, literal, nullable, Infer } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type ProgramAccountInfo = Infer<typeof ProgramAccountInfo>;
export const ProgramAccountInfo = type({
  programData: PublicKeyFromString,
});

export type ProgramAccount = Infer<typeof ProgramDataAccount>;
export const ProgramAccount = type({
  type: literal("program"),
  info: ProgramAccountInfo,
});

export type ProgramDataAccountInfo = Infer<typeof ProgramDataAccountInfo>;
export const ProgramDataAccountInfo = type({
  authority: nullable(PublicKeyFromString),
  // don't care about data yet
  slot: number(),
});

export type ProgramDataAccount = Infer<typeof ProgramDataAccount>;
export const ProgramDataAccount = type({
  type: literal("programData"),
  info: ProgramDataAccountInfo,
});
