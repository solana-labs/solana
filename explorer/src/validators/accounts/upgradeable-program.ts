/* eslint-disable @typescript-eslint/no-redeclare */

import {
  type,
  number,
  literal,
  nullable,
  Infer,
  union,
  coerce,
  create,
  any,
  string,
  tuple,
} from "superstruct";
import { ParsedInfo } from "validators";
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
  data: tuple([string(), literal("base64")]),
  slot: number(),
});

export type ProgramDataAccount = Infer<typeof ProgramDataAccount>;
export const ProgramDataAccount = type({
  type: literal("programData"),
  info: ProgramDataAccountInfo,
});

export type ProgramBufferAccountInfo = Infer<typeof ProgramBufferAccountInfo>;
export const ProgramBufferAccountInfo = type({
  authority: nullable(PublicKeyFromString),
  // don't care about data yet
});

export type ProgramBufferAccount = Infer<typeof ProgramBufferAccount>;
export const ProgramBufferAccount = type({
  type: literal("buffer"),
  info: ProgramBufferAccountInfo,
});

export type ProgramUninitializedAccountInfo = Infer<
  typeof ProgramUninitializedAccountInfo
>;
export const ProgramUninitializedAccountInfo = any();

export type ProgramUninitializedAccount = Infer<
  typeof ProgramUninitializedAccount
>;
export const ProgramUninitializedAccount = type({
  type: literal("uninitialized"),
  info: ProgramUninitializedAccountInfo,
});

export type UpgradeableLoaderAccount = Infer<typeof UpgradeableLoaderAccount>;
export const UpgradeableLoaderAccount = coerce(
  union([
    ProgramAccount,
    ProgramDataAccount,
    ProgramBufferAccount,
    ProgramUninitializedAccount,
  ]),
  ParsedInfo,
  (value) => {
    // Coercions like `PublicKeyFromString` are not applied within
    // union validators so we use this custom coercion as a workaround.
    switch (value.type) {
      case "program": {
        return {
          type: value.type,
          info: create(value.info, ProgramAccountInfo),
        };
      }
      case "programData": {
        return {
          type: value.type,
          info: create(value.info, ProgramDataAccountInfo),
        };
      }
      case "buffer": {
        return {
          type: value.type,
          info: create(value.info, ProgramBufferAccountInfo),
        };
      }
      case "uninitialized": {
        return {
          type: value.type,
          info: create(value.info, ProgramUninitializedAccountInfo),
        };
      }
      default: {
        throw new Error(`Unknown program account type: ${value.type}`);
      }
    }
  }
);
